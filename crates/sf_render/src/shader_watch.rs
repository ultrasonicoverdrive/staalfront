//! Debug-only shader hot-reload: polls the modification time of one WGSL
//! file. Compiled in debug builds only (release builds embed the shader and
//! contain no watcher).
//!
//! Polling a single file's mtime a few times per second costs one `stat`, needs
//! no extra dependency and behaves the same on Linux and Windows.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

pub struct ShaderWatch {
    path: PathBuf,
    last_mtime: Option<SystemTime>,
    next_check: Instant,
    interval: Duration,
}

impl ShaderWatch {
    /// Watch `path`; the file's current state counts as already loaded.
    pub fn new(path: PathBuf, interval: Duration) -> Self {
        let last_mtime = mtime(&path);
        Self {
            path,
            last_mtime,
            next_check: Instant::now() + interval,
            interval,
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// New source text if the file changed since the last call that returned
    /// `Some` (or since creation). A failed read is retried at the next poll.
    pub fn poll(&mut self) -> Option<String> {
        let now = Instant::now();
        if now < self.next_check {
            return None;
        }
        self.next_check = now + self.interval;
        let current = mtime(&self.path)?;
        if self.last_mtime == Some(current) {
            return None;
        }
        let text = std::fs::read_to_string(&self.path).ok()?;
        self.last_mtime = Some(current);
        Some(text)
    }
}

fn mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}
