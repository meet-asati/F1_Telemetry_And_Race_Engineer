//! Disk cache for synthesized speech, keyed by a stable hash of the TTS
//! engine/voice namespace and the text. Templated callouts repeat often, so
//! most rule callouts play from cache with near-zero latency.

use ai_sidecar::Pcm;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct TtsCache {
    dir: PathBuf,
}

/// FNV-1a 64-bit — stable across Rust versions and platforms.
pub fn stable_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl TtsCache {
    pub fn new(dir: impl AsRef<Path>) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir.as_ref())?;
        Ok(Self { dir: dir.as_ref().to_path_buf() })
    }

    fn path(&self, namespace: &str, text: &str) -> PathBuf {
        self.dir.join(format!("{:016x}.wav", stable_hash(&format!("{namespace}\u{1f}{}", text.trim()))))
    }

    pub fn get(&self, namespace: &str, text: &str) -> Option<Pcm> {
        let bytes = std::fs::read(self.path(namespace, text)).ok()?;
        Pcm::from_wav(&bytes).ok()
    }

    pub fn put(&self, namespace: &str, text: &str, pcm: &Pcm) -> anyhow::Result<()> {
        let path = self.path(namespace, text);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, pcm.to_wav()?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_and_namespace_isolation() {
        let dir = std::env::temp_dir().join(format!("tts-cache-test-{}", std::process::id()));
        let c = TtsCache::new(&dir).unwrap();
        let pcm = Pcm { samples: vec![0.1, 0.2, 0.3], sample_rate: 22050 };
        assert!(c.get("piper", "Box box").is_none());
        c.put("piper", "Box box", &pcm).unwrap();
        assert_eq!(c.get("piper", "Box box"), Some(pcm.clone()));
        assert_eq!(c.get("piper", "  Box box "), Some(pcm), "whitespace-insensitive");
        assert!(c.get("kokoro|af", "Box box").is_none());
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(stable_hash("abc"), 0xe71fa2190541574b);
    }
}
