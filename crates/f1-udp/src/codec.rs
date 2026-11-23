//! A symmetric binary codec: every packet layout is described once, as a
//! function over `&mut impl Codec`, and that single description is used both
//! to decode (with [`Reader`]) and to encode (with [`Writer`]).
//!
//! All F1 UDP structs are packed little-endian, so the codec is a plain
//! sequential cursor with no alignment handling.

use crate::DecodeError;

pub type Result<T> = std::result::Result<T, DecodeError>;

pub trait Codec {
    fn is_reader(&self) -> bool;
    fn pos(&self) -> usize;
    fn bytes(&mut self, v: &mut [u8]) -> Result<()>;
    /// Reader: skip `n` bytes. Writer: write `n` zero bytes.
    fn skip(&mut self, n: usize) -> Result<()>;

    fn u8(&mut self, v: &mut u8) -> Result<()> {
        let mut b = [*v];
        self.bytes(&mut b)?;
        *v = b[0];
        Ok(())
    }
    fn i8(&mut self, v: &mut i8) -> Result<()> {
        let mut b = v.to_le_bytes();
        self.bytes(&mut b)?;
        *v = i8::from_le_bytes(b);
        Ok(())
    }
    fn bool(&mut self, v: &mut bool) -> Result<()> {
        let mut b = u8::from(*v);
        self.u8(&mut b)?;
        *v = b != 0;
        Ok(())
    }
    fn u16(&mut self, v: &mut u16) -> Result<()> {
        let mut b = v.to_le_bytes();
        self.bytes(&mut b)?;
        *v = u16::from_le_bytes(b);
        Ok(())
    }
    fn i16(&mut self, v: &mut i16) -> Result<()> {
        let mut b = v.to_le_bytes();
        self.bytes(&mut b)?;
        *v = i16::from_le_bytes(b);
        Ok(())
    }
    fn u32(&mut self, v: &mut u32) -> Result<()> {
        let mut b = v.to_le_bytes();
        self.bytes(&mut b)?;
        *v = u32::from_le_bytes(b);
        Ok(())
    }
    fn u64(&mut self, v: &mut u64) -> Result<()> {
        let mut b = v.to_le_bytes();
        self.bytes(&mut b)?;
        *v = u64::from_le_bytes(b);
        Ok(())
    }
    fn f32(&mut self, v: &mut f32) -> Result<()> {
        let mut b = v.to_le_bytes();
        self.bytes(&mut b)?;
        *v = f32::from_le_bytes(b);
        Ok(())
    }
    fn f64(&mut self, v: &mut f64) -> Result<()> {
        let mut b = v.to_le_bytes();
        self.bytes(&mut b)?;
        *v = f64::from_le_bytes(b);
        Ok(())
    }
}

/// Decodes from a byte slice.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn need(&self, n: usize) -> Result<()> {
        if self.pos + n > self.buf.len() {
            Err(DecodeError::Truncated { needed: self.pos + n, got: self.buf.len() })
        } else {
            Ok(())
        }
    }
}

impl Codec for Reader<'_> {
    fn is_reader(&self) -> bool {
        true
    }
    fn pos(&self) -> usize {
        self.pos
    }
    fn bytes(&mut self, v: &mut [u8]) -> Result<()> {
        self.need(v.len())?;
        v.copy_from_slice(&self.buf[self.pos..self.pos + v.len()]);
        self.pos += v.len();
        Ok(())
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.need(n)?;
        self.pos += n;
        Ok(())
    }
}

/// Encodes into a growable buffer.
#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Codec for Writer {
    fn is_reader(&self) -> bool {
        false
    }
    fn pos(&self) -> usize {
        self.buf.len()
    }
    fn bytes(&mut self, v: &mut [u8]) -> Result<()> {
        self.buf.extend_from_slice(v);
        Ok(())
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.buf.resize(self.buf.len() + n, 0);
        Ok(())
    }
}

/// Game wheel order is `RL, RR, FL, FR`; the model order is `FL, FR, RL, RR`.
/// Visiting model slots in game order makes the reorder symmetric for
/// both reading and writing.
const GAME_TO_MODEL: [usize; 4] = [2, 3, 0, 1];

pub fn wheels_u8<C: Codec>(c: &mut C, v: &mut [u8; 4]) -> Result<()> {
    for mi in GAME_TO_MODEL {
        c.u8(&mut v[mi])?;
    }
    Ok(())
}
pub fn wheels_u16<C: Codec>(c: &mut C, v: &mut [u16; 4]) -> Result<()> {
    for mi in GAME_TO_MODEL {
        c.u16(&mut v[mi])?;
    }
    Ok(())
}
pub fn wheels_f32<C: Codec>(c: &mut C, v: &mut [f32; 4]) -> Result<()> {
    for mi in GAME_TO_MODEL {
        c.f32(&mut v[mi])?;
    }
    Ok(())
}

/// A time split into a `u16` millisecond part and an optional `u8` minutes
/// part (F1 23+ for sectors, F1 24+ for deltas). Stored as total ms.
pub fn split_time<C: Codec>(c: &mut C, v: &mut u32, has_minutes: bool) -> Result<()> {
    let mut ms: u16 = if has_minutes { (*v % 60_000) as u16 } else { (*v).min(u16::MAX as u32) as u16 };
    let mut minutes: u8 = if has_minutes { (*v / 60_000).min(255) as u8 } else { 0 };
    c.u16(&mut ms)?;
    if has_minutes {
        c.u8(&mut minutes)?;
    }
    if c.is_reader() {
        *v = u32::from(minutes) * 60_000 + u32::from(ms);
    }
    Ok(())
}

/// Fixed-width, NUL-padded UTF-8 string.
pub fn fixed_str<C: Codec>(c: &mut C, v: &mut String, len: usize) -> Result<()> {
    let mut raw = vec![0u8; len];
    if !c.is_reader() {
        let bytes = v.as_bytes();
        let n = bytes.len().min(len.saturating_sub(1));
        raw[..n].copy_from_slice(&bytes[..n]);
    }
    c.bytes(&mut raw)?;
    if c.is_reader() {
        let end = raw.iter().position(|&b| b == 0).unwrap_or(len);
        *v = String::from_utf8_lossy(&raw[..end]).into_owned();
    }
    Ok(())
}

/// A count byte followed by `max` fixed-size slots, of which the first
/// `count` are populated (marshal zones, forecast samples, ...).
pub fn counted_slots<C: Codec, T: Default>(
    c: &mut C,
    items: &mut Vec<T>,
    max: usize,
    slot_size: usize,
    mut each: impl FnMut(&mut C, &mut T) -> Result<()>,
) -> Result<()> {
    let mut count = items.len().min(max) as u8;
    c.u8(&mut count)?;
    let count = (count as usize).min(max);
    if c.is_reader() {
        items.clear();
        items.resize_with(count, T::default);
    }
    for item in items.iter_mut().take(count) {
        each(c, item)?;
    }
    c.skip((max - count) * slot_size)
}
