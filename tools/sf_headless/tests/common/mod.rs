//! Helpers shared by the integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// The shipped scenarios (`assets/scenarios`).
pub fn scenario_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/scenarios")
        .join(name)
}

/// A scratch directory under the system temp dir, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> TempDir {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("sf_headless_{label}_{}_{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The part of a log that must be equal on every machine: everything before
/// the `host` block.
pub fn deterministic_part(log: &str) -> &str {
    let at = log.find("  \"host\": {").expect("the log has a host block");
    &log[..at]
}

/// The value of a top-level string field of a log, like `final_checksum`.
pub fn string_field<'a>(log: &'a str, key: &str) -> &'a str {
    let needle = format!("\"{key}\": \"");
    let start = log.find(&needle).expect("field present") + needle.len();
    let end = log[start..].find('"').expect("closing quote");
    &log[start..start + end]
}
