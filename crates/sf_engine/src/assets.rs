//! Asset root resolution and (debug builds only) RON hot-reload.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the asset directory.
pub const ASSET_ENV: &str = "SF_ASSETS";

/// Location of the `assets/` directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetRoot {
    root: PathBuf,
}

impl AssetRoot {
    /// Use an explicit directory (no checks).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolve from the environment and the executable location:
    /// 1. `SF_ASSETS` if set,
    /// 2. `assets/` next to the executable, or in any parent directory of it
    ///    (finds the workspace `assets/` for `target/debug/sf_game`),
    /// 3. `assets/` in the current directory,
    /// 4. otherwise `assets/` next to the executable (may not exist).
    pub fn resolve() -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        let cwd = std::env::current_dir().ok();
        Self::resolve_from(
            std::env::var_os(ASSET_ENV),
            exe_dir.as_deref(),
            cwd.as_deref(),
        )
    }

    /// Pure core of [`AssetRoot::resolve`] with injected environment.
    pub fn resolve_from(env: Option<OsString>, exe_dir: Option<&Path>, cwd: Option<&Path>) -> Self {
        if let Some(v) = env.filter(|v| !v.is_empty()) {
            return Self::new(PathBuf::from(v));
        }
        if let Some(dir) = exe_dir {
            for ancestor in dir.ancestors() {
                let candidate = ancestor.join("assets");
                if candidate.is_dir() {
                    return Self::new(candidate);
                }
            }
        }
        if let Some(cwd) = cwd {
            let candidate = cwd.join("assets");
            if candidate.is_dir() {
                return Self::new(candidate);
            }
        }
        Self::new(exe_dir.unwrap_or_else(|| Path::new(".")).join("assets"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Path of an asset given relative to the root, e.g. `config/input.ron`.
    pub fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }

    /// Path of the input bindings file.
    pub fn input_bindings(&self) -> PathBuf {
        self.path("config/input.ron")
    }
}

#[cfg(debug_assertions)]
pub use hot::HotReload;

/// File watcher for RON assets. Compiled in debug builds only, so release
/// builds contain no watcher code.
#[cfg(debug_assertions)]
mod hot {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{Receiver, channel};

    use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

    pub struct HotReload {
        // Held to keep the watcher alive.
        _watcher: RecommendedWatcher,
        rx: Receiver<notify::Result<notify::Event>>,
    }

    impl HotReload {
        /// Watch `root` recursively. Errors if the directory cannot be watched.
        pub fn new(root: &Path) -> notify::Result<Self> {
            let (tx, rx) = channel();
            let mut watcher = notify::recommended_watcher(tx)?;
            watcher.watch(root, RecursiveMode::Recursive)?;
            Ok(Self {
                _watcher: watcher,
                rx,
            })
        }

        /// Changed `.ron` files since the last call (deduplicated, sorted).
        pub fn poll(&self) -> Vec<PathBuf> {
            let mut changed = BTreeSet::new();
            while let Ok(ev) = self.rx.try_recv() {
                let Ok(ev) = ev else { continue };
                if !matches!(ev.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    continue;
                }
                changed.extend(ev.paths.into_iter().filter(|p| is_ron(p)));
            }
            changed.into_iter().collect()
        }
    }

    fn is_ron(p: &Path) -> bool {
        p.extension().is_some_and(|e| e == "ron")
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::{Duration, Instant};

        #[test]
        fn is_ron_filters_by_extension() {
            assert!(is_ron(Path::new("a/b.ron")));
            assert!(!is_ron(Path::new("a/b.png")));
            assert!(!is_ron(Path::new("a/ron")));
        }

        #[test]
        fn detects_a_changed_ron_file() {
            let dir = std::env::temp_dir().join(format!("sf_engine_hot_{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let file = dir.join("x.ron");
            std::fs::write(&file, "()").unwrap();
            let hot = HotReload::new(&dir).unwrap();
            std::fs::write(&file, "(1)").unwrap();
            std::fs::write(dir.join("ignored.txt"), "x").unwrap();

            let deadline = Instant::now() + Duration::from_secs(10);
            let mut seen = Vec::new();
            while seen.is_empty() && Instant::now() < deadline {
                seen = hot.poll();
                std::thread::sleep(Duration::from_millis(50));
            }
            std::fs::remove_dir_all(&dir).ok();
            assert_eq!(seen.len(), 1, "{seen:?}");
            assert!(seen[0].ends_with("x.ron"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("sf_engine_assets_{}_{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn env_override_wins() {
        let r = AssetRoot::resolve_from(Some("/somewhere".into()), Some(Path::new("/x")), None);
        assert_eq!(r.root(), Path::new("/somewhere"));
    }

    #[test]
    fn empty_env_is_ignored() {
        let d = tmp("empty_env");
        std::fs::create_dir_all(d.join("assets")).unwrap();
        let r = AssetRoot::resolve_from(Some("".into()), Some(&d), None);
        assert_eq!(r.root(), d.join("assets"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn finds_assets_in_a_parent_of_the_exe_dir() {
        let d = tmp("parent");
        std::fs::create_dir_all(d.join("assets")).unwrap();
        let exe_dir = d.join("target").join("debug");
        std::fs::create_dir_all(&exe_dir).unwrap();
        let r = AssetRoot::resolve_from(None, Some(&exe_dir), None);
        assert_eq!(r.root(), d.join("assets"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn falls_back_to_cwd_then_next_to_exe() {
        let cwd = tmp("cwd");
        std::fs::create_dir_all(cwd.join("assets")).unwrap();
        let exe = tmp("exe_no_assets_sub").join("deep");
        std::fs::create_dir_all(&exe).unwrap();
        // The temp dir itself must not contain an `assets` directory above `exe`.
        let r = AssetRoot::resolve_from(None, Some(&exe), Some(&cwd));
        assert_eq!(r.root(), cwd.join("assets"));

        let r = AssetRoot::resolve_from(None, Some(&exe), None);
        assert_eq!(r.root(), exe.join("assets"));
        std::fs::remove_dir_all(&cwd).ok();
        std::fs::remove_dir_all(exe.parent().unwrap()).ok();
    }

    #[test]
    fn path_helpers() {
        let r = AssetRoot::new("/a");
        assert_eq!(r.path("x/y.ron"), Path::new("/a/x/y.ron"));
        assert_eq!(r.input_bindings(), Path::new("/a/config/input.ron"));
    }
}
