//! Noticing that files changed, for the live preview (WP12.6).
//!
//! Plain **polling**, no file-watch crate: every [`POLL_INTERVAL_MS`] the
//! watched files are looked at (size, modification time and a hash of the
//! content) and compared with the last look. `notify` is in `Cargo.lock`, but
//! only behind `sf_engine`'s debug asset hot-reload; a poller needs no thread,
//! behaves the same on Linux and Windows, sees a rename-style save without
//! any special case (the *path* is looked at, not an inode or a directory
//! event), and can be driven by a fake clock in tests. For the handful of files
//! of a scene that is cheap.
//!
//! * The **content hash** is part of the comparison because the modification
//!   time can be coarse (FAT: two seconds; Windows may update it late for a
//!   file that is still open), so two saves in a row can look alike in size
//!   and time. Files above [`HASH_LIMIT`] are compared by size and time only.
//!   A save that changes nothing but the time (`touch`) counts as a change on
//!   purpose: it is the way to force a reload.
//! * A missing file is a state of its own (`Missing`), so "deleted, then
//!   renamed into place" by an editor is two changes that the debounce merges,
//!   and a picture that does not exist yet triggers a reload the moment the
//!   artist saves it.
//! * [`Debounce`] waits until the changes stop: editors write in several steps
//!   (temp file, rename, a second touch), and a half-written file must not be
//!   loaded.
//!
//! Both take the time as a plain number of milliseconds from the caller, so
//! the tests use a fake clock.

use std::collections::BTreeMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// How often the files are looked at.
pub const POLL_INTERVAL_MS: u64 = 250;
/// How long the files must stay quiet after the last change before a reload.
pub const DEBOUNCE_MS: u64 = 150;
/// Files larger than this are compared by size and time only (no hashing).
pub const HASH_LIMIT: u64 = 32 * 1024 * 1024;

/// What a file looked like at one look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stamp {
    /// Not there (or not a file).
    Missing,
    Present {
        len: u64,
        /// Modification time, nanoseconds since the epoch (`None` if the
        /// platform cannot say).
        mtime: Option<u128>,
        /// Hash of the content (`None` for a file above [`HASH_LIMIT`] or one
        /// that could not be read right now).
        hash: Option<u64>,
    },
}

impl Stamp {
    /// Look at `path` now.
    pub fn of(path: &Path) -> Stamp {
        let Ok(meta) = std::fs::metadata(path) else {
            return Stamp::Missing;
        };
        if !meta.is_file() {
            return Stamp::Missing;
        }
        let len = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos());
        let hash = if len <= HASH_LIMIT {
            std::fs::read(path).ok().map(|bytes| hash_bytes(&bytes))
        } else {
            None
        };
        Stamp::Present { len, mtime, hash }
    }
}

/// A hash of some bytes. `DefaultHasher::new()` has fixed keys, so the value is
/// the same for the same bytes within one run (all that is compared).
pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    h.write(bytes);
    h.finish()
}

/// "Wait until the changes stop." Feed it every observed change with
/// [`Debounce::change`]; [`Debounce::due`] says, once, when no further change
/// came for `quiet_ms`.
#[derive(Clone, Debug)]
pub struct Debounce {
    quiet_ms: u64,
    last_change: Option<u64>,
}

impl Debounce {
    pub fn new(quiet_ms: u64) -> Debounce {
        Debounce {
            quiet_ms,
            last_change: None,
        }
    }

    /// A change was seen at `now_ms`; the quiet period starts over.
    pub fn change(&mut self, now_ms: u64) {
        self.last_change = Some(now_ms);
    }

    /// Whether a change is waiting to settle.
    pub fn pending(&self) -> bool {
        self.last_change.is_some()
    }

    /// `true` once, when the last change is at least `quiet_ms` ago.
    pub fn due(&mut self, now_ms: u64) -> bool {
        match self.last_change {
            Some(t) if now_ms.saturating_sub(t) >= self.quiet_ms => {
                self.last_change = None;
                true
            }
            _ => false,
        }
    }
}

/// A set of files looked at on a schedule.
#[derive(Clone, Debug)]
pub struct Poller {
    interval_ms: u64,
    next_poll_ms: u64,
    files: BTreeMap<PathBuf, Stamp>,
}

impl Poller {
    pub fn new(interval_ms: u64) -> Poller {
        Poller {
            interval_ms,
            next_poll_ms: 0,
            files: BTreeMap::new(),
        }
    }

    /// Watch exactly these files. A file that was already watched keeps its last
    /// stamp (so a change that is still to be noticed is not lost); a new one
    /// is looked at now and is the baseline; the rest are dropped.
    pub fn set_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut next = BTreeMap::new();
        for path in paths {
            let stamp = match self.files.get(&path) {
                Some(s) => *s,
                None => Stamp::of(&path),
            };
            next.insert(path, stamp);
        }
        self.files = next;
    }

    /// The files being watched, in path order.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.files.keys().map(PathBuf::as_path)
    }

    /// Take a fresh look at every file and make it the baseline, without
    /// reporting anything.
    pub fn rebaseline(&mut self) {
        for (path, stamp) in &mut self.files {
            *stamp = Stamp::of(path);
        }
    }

    /// Look at the files if the interval has passed since the last look. `true`
    /// if any of them differs from the look before (the new look becomes the
    /// baseline, so each change is reported once).
    pub fn poll(&mut self, now_ms: u64) -> bool {
        if now_ms < self.next_poll_ms {
            return false;
        }
        self.next_poll_ms = now_ms.saturating_add(self.interval_ms);
        let mut changed = false;
        for (path, old) in &mut self.files {
            let new = Stamp::of(path);
            if new != *old {
                *old = new;
                changed = true;
            }
        }
        changed
    }
}

/// Poller and debounce together: [`SceneWatcher::tick`] says when to reload.
#[derive(Clone, Debug)]
pub struct SceneWatcher {
    poller: Poller,
    debounce: Debounce,
}

impl SceneWatcher {
    /// The standard rhythm: look every [`POLL_INTERVAL_MS`], reload
    /// [`DEBOUNCE_MS`] after the last change.
    pub fn new() -> SceneWatcher {
        SceneWatcher::with_timing(POLL_INTERVAL_MS, DEBOUNCE_MS)
    }

    pub fn with_timing(interval_ms: u64, debounce_ms: u64) -> SceneWatcher {
        SceneWatcher {
            poller: Poller::new(interval_ms),
            debounce: Debounce::new(debounce_ms),
        }
    }

    pub fn set_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.poller.set_paths(paths);
    }

    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.poller.paths()
    }

    /// Call as often as you like (every frame) with a monotonic millisecond
    /// clock. `true` once per settled burst of changes: time to reload.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if self.poller.poll(now_ms) {
            self.debounce.change(now_ms);
        }
        let due = self.debounce.due(now_ms);
        if due {
            // The caller is about to read the files as they are now: whatever
            // the next look finds different from this is a new change. (A save
            // that is only seen by the next look, after it was already read,
            // would otherwise reload the same content a second time.)
            self.poller.rebaseline();
        }
        due
    }

    /// A change was seen and the reload is still waiting for it to settle.
    pub fn pending(&self) -> bool {
        self.debounce.pending()
    }
}

impl Default for SceneWatcher {
    fn default() -> SceneWatcher {
        SceneWatcher::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fresh directory in the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static N: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "sf_cutscene-{tag}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn debounce_waits_for_quiet_and_fires_once() {
        let mut d = Debounce::new(150);
        assert!(!d.due(0), "nothing changed");
        d.change(1000);
        assert!(d.pending());
        assert!(!d.due(1000));
        assert!(!d.due(1149));
        assert!(d.due(1150), "150 ms after the change");
        assert!(!d.pending());
        assert!(!d.due(5000), "only once");
    }

    #[test]
    fn debounce_starts_over_with_every_change() {
        let mut d = Debounce::new(150);
        d.change(1000);
        d.change(1100); // the second step of a save
        assert!(!d.due(1200), "only 100 ms since the last change");
        d.change(1240);
        assert!(!d.due(1389));
        assert!(d.due(1390));
        assert!(!d.due(1400));
    }

    #[test]
    fn debounce_survives_a_clock_that_goes_back() {
        let mut d = Debounce::new(150);
        d.change(1000);
        assert!(!d.due(10), "saturating, no underflow");
    }

    #[test]
    fn the_poller_looks_on_its_schedule_and_reports_a_change_once() {
        let dir = TempDir::new("poll");
        let f = dir.0.join("a.txt");
        std::fs::write(&f, "one").unwrap();
        let mut p = Poller::new(250);
        p.set_paths([f.clone()]);
        assert!(!p.poll(0), "the baseline is not a change");
        std::fs::write(&f, "two!").unwrap();
        assert!(!p.poll(100), "too early: no look");
        assert!(!p.poll(249));
        assert!(p.poll(250), "the change is seen at the next look");
        assert!(!p.poll(500), "and only once");
    }

    #[test]
    fn a_same_size_same_second_edit_is_caught_by_the_content_hash() {
        let dir = TempDir::new("hash");
        let f = dir.0.join("a.txt");
        std::fs::write(&f, "duration_ms: 1500").unwrap();
        let mut p = Poller::new(1);
        p.set_paths([f.clone()]);
        assert!(!p.poll(0));
        // Same length; put the old modification time back, as a filesystem with
        // coarse time stamps would show it.
        let old_mtime = std::fs::metadata(&f).unwrap().modified().unwrap();
        std::fs::write(&f, "duration_ms: 1600").unwrap();
        let file = std::fs::File::options().write(true).open(&f).unwrap();
        file.set_modified(old_mtime).unwrap();
        drop(file);
        assert_eq!(
            std::fs::metadata(&f).unwrap().len(),
            "duration_ms: 1500".len() as u64
        );
        assert!(p.poll(10), "size and time alike, content not");
    }

    #[test]
    fn a_missing_file_is_a_state_and_its_appearing_is_a_change() {
        let dir = TempDir::new("missing");
        let f = dir.0.join("later.png");
        let mut p = Poller::new(1);
        p.set_paths([f.clone()]);
        assert!(!p.poll(0));
        assert!(!p.poll(10), "still not there");
        std::fs::write(&f, [1, 2, 3]).unwrap();
        assert!(p.poll(20), "appeared");
        std::fs::remove_file(&f).unwrap();
        assert!(p.poll(30), "disappeared");
    }

    #[test]
    fn a_rename_style_save_is_seen_through_the_path() {
        let dir = TempDir::new("rename");
        let f = dir.0.join("scene.ron");
        std::fs::write(&f, "old").unwrap();
        let mut p = Poller::new(1);
        p.set_paths([f.clone()]);
        assert!(!p.poll(0));
        let tmp = dir.0.join("scene.ron.tmp12345");
        std::fs::write(&tmp, "new content").unwrap();
        assert!(!p.poll(10), "the temp file is not watched");
        std::fs::rename(&tmp, &f).unwrap();
        assert!(p.poll(20));
    }

    #[test]
    fn set_paths_keeps_old_stamps_and_baselines_new_files() {
        let dir = TempDir::new("setpaths");
        let (a, b) = (dir.0.join("a"), dir.0.join("b"));
        std::fs::write(&a, "1").unwrap();
        std::fs::write(&b, "1").unwrap();
        let mut p = Poller::new(1);
        p.set_paths([a.clone()]);
        std::fs::write(&a, "22").unwrap(); // changed, not yet noticed
        p.set_paths([a.clone(), b.clone()]);
        assert!(p.poll(0), "the change in `a` is still reported");
        assert!(!p.poll(10));
        std::fs::write(&b, "22").unwrap();
        assert!(p.poll(20));
        p.set_paths([a.clone()]);
        std::fs::write(&b, "333").unwrap();
        assert!(!p.poll(30), "b is no longer watched");
        assert_eq!(p.paths().count(), 1);
    }

    #[test]
    fn the_watcher_reloads_once_after_a_burst_of_saves() {
        let dir = TempDir::new("burst");
        let f = dir.0.join("scene.ron");
        std::fs::write(&f, "v1").unwrap();
        let mut w = SceneWatcher::new();
        w.set_paths([f.clone()]);
        assert!(!w.tick(1000), "first look: the baseline");
        std::fs::write(&f, "v2").unwrap();
        assert!(!w.tick(1100), "no look yet (every 250 ms)");
        assert!(!w.tick(1250), "seen, now settling");
        assert!(w.pending());
        assert!(!w.tick(1399));
        assert!(w.tick(1400), "150 ms after it was seen");
        assert!(!w.tick(1500));
        assert!(!w.tick(2000), "nothing new, nothing to do");
    }

    #[test]
    fn a_save_that_was_already_read_does_not_reload_again() {
        let dir = TempDir::new("twice");
        let f = dir.0.join("scene.ron");
        std::fs::write(&f, "v1").unwrap();
        let mut w = SceneWatcher::new();
        w.set_paths([f.clone()]);
        assert!(!w.tick(0));
        std::fs::remove_file(&f).unwrap();
        assert!(!w.tick(250), "seen gone, settling");
        std::fs::write(&f, "v2").unwrap(); // back before the 150 ms are over
        assert!(w.tick(400), "reload: the file is read as v2");
        assert!(!w.tick(500));
        assert!(!w.tick(1000), "the next look finds nothing new");
        std::fs::write(&f, "v3").unwrap();
        assert!(!w.tick(1250));
        assert!(w.tick(1400), "a real later change still reloads");
    }

    #[test]
    fn a_save_in_two_steps_waits_for_the_second_step() {
        let dir = TempDir::new("twostep");
        let f = dir.0.join("scene.ron");
        std::fs::write(&f, "v1").unwrap();
        let mut w = SceneWatcher::with_timing(100, 150);
        w.set_paths([f.clone()]);
        assert!(!w.tick(0));
        std::fs::write(&f, "half").unwrap(); // the editor truncates and writes
        assert!(!w.tick(100));
        std::fs::write(&f, "the whole file").unwrap(); // and finishes
        assert!(!w.tick(200), "changed again: the wait starts over");
        assert!(!w.tick(340));
        assert!(w.tick(350));
    }
}
