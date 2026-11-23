//! Raw datagram capture files.
//!
//! Layout (little-endian):
//! ```text
//! file header : b"F1PW" | u16 version (=1) | u16 packet format (0 = unknown/mixed)
//! record      : u64 t_us since capture start | u16 len | len bytes payload
//! ```

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::Instant;

pub const MAGIC: &[u8; 4] = b"F1PW";
pub const VERSION: u16 = 1;

pub struct Recorder {
    out: BufWriter<File>,
    start: Instant,
    records: u64,
}

impl Recorder {
    pub fn create(path: impl AsRef<Path>, packet_format: u16) -> io::Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(MAGIC)?;
        out.write_all(&VERSION.to_le_bytes())?;
        out.write_all(&packet_format.to_le_bytes())?;
        Ok(Self { out, start: Instant::now(), records: 0 })
    }

    /// Record a datagram, timestamped relative to recorder creation.
    pub fn record(&mut self, payload: &[u8]) -> io::Result<()> {
        let t_us = self.start.elapsed().as_micros() as u64;
        self.record_at(t_us, payload)
    }

    pub fn record_at(&mut self, t_us: u64, payload: &[u8]) -> io::Result<()> {
        let len = u16::try_from(payload.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "datagram > 65535 bytes"))?;
        self.out.write_all(&t_us.to_le_bytes())?;
        self.out.write_all(&len.to_le_bytes())?;
        self.out.write_all(payload)?;
        self.records += 1;
        Ok(())
    }

    pub fn records(&self) -> u64 {
        self.records
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.out.flush();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub t_us: u64,
    pub payload: Vec<u8>,
}

pub struct CaptureReader<R: Read> {
    input: R,
    pub packet_format: u16,
}

impl CaptureReader<BufReader<File>> {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::new(BufReader::new(File::open(path)?))
    }
}

impl<R: Read> CaptureReader<R> {
    pub fn new(mut input: R) -> io::Result<Self> {
        let mut hdr = [0u8; 8];
        input.read_exact(&mut hdr)?;
        if &hdr[0..4] != MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not an F1PW capture file"));
        }
        let version = u16::from_le_bytes([hdr[4], hdr[5]]);
        if version != VERSION {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unsupported capture version {version}")));
        }
        Ok(Self { input, packet_format: u16::from_le_bytes([hdr[6], hdr[7]]) })
    }

    /// Next record, `Ok(None)` at a clean end of file. A truncated final
    /// record (e.g. the process was killed mid-write) is treated as EOF.
    pub fn next_record(&mut self) -> io::Result<Option<Record>> {
        let mut head = [0u8; 10];
        match read_full(&mut self.input, &mut head)? {
            0 => return Ok(None),
            10 => {}
            _ => return Ok(None),
        }
        let t_us = u64::from_le_bytes(head[0..8].try_into().unwrap());
        let len = u16::from_le_bytes([head[8], head[9]]) as usize;
        let mut payload = vec![0u8; len];
        if read_full(&mut self.input, &mut payload)? < len {
            return Ok(None);
        }
        Ok(Some(Record { t_us, payload }))
    }
}

impl<R: Read> Iterator for CaptureReader<R> {
    type Item = io::Result<Record>;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_record().transpose()
    }
}

fn read_full<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_read_back() {
        let dir = std::env::temp_dir().join(format!("f1pw-test-{}", std::process::id()));
        let path = dir.join("cap.bin");
        {
            let mut rec = Recorder::create(&path, 2024).unwrap();
            rec.record_at(0, &[1, 2, 3]).unwrap();
            rec.record_at(50_000, &[4; 1000]).unwrap();
            rec.flush().unwrap();
        }
        // Append a partial record to simulate a crash mid-write.
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(&[9, 9, 9]).unwrap();
        }
        let reader = CaptureReader::open(&path).unwrap();
        assert_eq!(reader.packet_format, 2024);
        let recs: Vec<_> = reader.map(|r| r.unwrap()).collect();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0], Record { t_us: 0, payload: vec![1, 2, 3] });
        assert_eq!(recs[1].t_us, 50_000);
        assert_eq!(recs[1].payload.len(), 1000);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_bad_magic() {
        let data = b"NOPE\x01\x00\x00\x00".to_vec();
        assert!(CaptureReader::new(&data[..]).is_err());
    }
}
