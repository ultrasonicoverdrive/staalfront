//! Noticing that files changed, for the live preview (WP12.6).
//!
//! Plain **polling**, no file-watch crate: every [`POLL_INTERVAL_MS`] the
//! watched files are looked at (size and modification time, and sometimes a
//! hash of the content, see below) and compared with the last look. `notify` is in `Cargo.lock`, but
//! only behind `sf_engine`'s debug asset hot-reload; a poller needs no thread,
//! behaves the same on Linux and Windows, sees a rename-style save without
//! any special case (the *path* is looked at, not an inode or a directory
//! event), and can be driven by a fake clock in tests. For the handful of files
//! of a scene that is cheap.
//!
//! * **Size and modification time first.** A file whose size or time differs
//!   from the last look has changed. Reading and hashing every file at every
//!   look would be wasteful, so the **content hash** is only taken (a) when
//!   size or time changed, to remember it, and (b) while the file's time is
//!   within [`RECENT_MTIME_MS`] of now: the time can be coarse (FAT: two
//!   seconds; Windows may update it late for a file that is still open), so a
//!   second save right after the first can look alike in size and time, and the
//!   hash tells them apart. A file that has not been touched for two seconds is
//!   only `stat`ed. Files above [`HASH_LIMIT`] are never hashed. A save that
//!   changes nothing but the time (`touch`) counts as a change on purpose: it is
//!   the way to force a reload.
//! * A missing file is a state of its own (`Missing`), so "deleted, then
//!   renamed into place" by an editor is two changes that the debounce merges,
//!   and a picture that does not exist yet triggers a reload the moment the
//!   artist saves it.
//! * [`Debounce`] waits until the changes stop: editors write in several steps
//!   (temp file, rename, a second touch), and a half-written file must not be
//!   loaded. The quiet period is **at least one whole poll interval**
//!   ([`SceneWatcher::with_timing`]); otherwise the reload would fire before the
//!   next look and a second write step would never be seen. So a reload comes
//!   after the first look that finds nothing new: 0.25 to 0.5 s after a save
//!   at the defaults.
//!
//! Both take the time as a plain number of milliseconds from the caller, so
//! the tests use a fake clock.

use std::collections::BTreeMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// How often the files are looked at.
pub const POLL_INTERVAL_MS: u64 = 250;
/// How long the files must stay quiet after the last change before a reload.
/// At the default poll interval the quiet period is raised to one interval
/// ([`SceneWatcher::with_timing`]), so this only matters with a shorter poll.
pub const DEBOUNCE_MS: u64 = 150;
/// A file whose modification time is this close to now is hashed at every look
/// (coarse time stamps could hide a second save).
pub const RECENT_MTIME_MS: u64 = 2000;
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
    /// Look at `path` now, hash included.
    pub fn of(path: &Path) -> Stamp {
        match stat(path) {
            None => Stamp::Missing,
            Some((len, mtime)) => Stamp::Present {
                len,
                mtime,
                hash: hash_file(path, len),
            },
        }
    }
}

/// Size and modification time (nanoseconds since the epoch) of a file; `None`
/// if it is not there or not a file.
fn stat(path: &Path) -> Option<(u64, Option<u128>)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos());
    Some((meta.len(), mtime))
}

/// The content hash, unless the file is above [`HASH_LIMIT`] or cannot be read
/// right now (an editor may hold it).
fn hash_file(path: &Path, len: u64) -> Option<u64> {
    if len > HASH_LIMIT {
        return None;
    }
    std::fs::read(path).ok().map(|bytes| hash_bytes(&bytes))
}

/// Whether a file with this modification time is "recent" (or its time is
/// unknown or in the future, which cannot be trusted either).
fn recent(mtime: Option<u128>, now: SystemTime) -> bool {
    let Some(m) = mtime else { return true };
    let now_ns = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    now_ns < m + u128::from(RECENT_MTIME_MS) * 1_000_000
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
    hashes: u64,
}

impl Poller {
    pub fn new(interval_ms: u64) -> Poller {
        Poller {
            interval_ms,
            next_poll_ms: 0,
            files: BTreeMap::new(),
            hashes: 0,
        }
    }

    /// How many files were hashed so far (tests: an old, untouched file must
    /// cost a `stat` per look, not a read).
    pub fn hashes_computed(&self) -> u64 {
        self.hashes
    }

    /// A full look (with hash) at `path`.
    fn stamp(&mut self, path: &Path) -> Stamp {
        let s = Stamp::of(path);
        if matches!(s, Stamp::Present { hash: Some(_), .. }) {
            self.hashes += 1;
        }
        s
    }

    /// Watch exactly these files. A file that was already watched keeps its last
    /// stamp (so a change that is still to be noticed is not lost); a new one
    /// is looked at now and is the baseline; the rest are dropped.
    pub fn set_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut next = BTreeMap::new();
        for path in paths {
            let stamp = match self.files.get(&path) {
                Some(s) => *s,
                None => self.stamp(&path),
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
        let paths: Vec<PathBuf> = self.files.keys().cloned().collect();
        for path in paths {
            let stamp = self.stamp(&path);
            self.files.insert(path, stamp);
        }
    }

    /// Look at the files if the interval has passed since the last look. `true`
    /// if any of them differs from the look before (the new look becomes the
    /// baseline, so each change is reported once). Size and modification time
    /// are compared first; the content is hashed only for a file that changed
    /// or whose time is recent (see the module docs).
    pub fn poll(&mut self, now_ms: u64) -> bool {
        if now_ms < self.next_poll_ms {
            return false;
        }
        self.next_poll_ms = now_ms.saturating_add(self.interval_ms);
        let wall = SystemTime::now();
        let mut changed = false;
        let paths: Vec<PathBuf> = self.files.keys().cloned().collect();
        for path in paths {
            let old = self.files[&path];
            let new = match (old, stat(&path)) {
                (Stamp::Missing, None) => old,
                (_, None) => Stamp::Missing,
                (Stamp::Missing, Some(_)) => self.stamp(&path),
                (
                    Stamp::Present {
                        len: old_len,
                        mtime: old_mtime,
                        hash: old_hash,
                    },
                    Some((len, mtime)),
                ) => {
                    if (len, mtime) != (old_len, old_mtime) {
                        // Changed: remember what it holds now.
                        self.stamp(&path)
                    } else if recent(mtime, wall) {
                        // Same size and time, but the time may be too coarse
                        // to tell: look at the content.
                        let hash = hash_file(&path, len);
                        self.hashes += u64::from(hash.is_some());
                        Stamp::Present { len, mtime, hash }
                    } else {
                        // Untouched for a while: a `stat` was all it cost.
                        Stamp::Present {
                            len,
                            mtime,
                            hash: old_hash,
                        }
                    }
                }
            };
            if new != old {
                self.files.insert(path, new);
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
    /// The standard rhythm: look every [`POLL_INTERVAL_MS`], reload once a whole
    /// interval has passed without a new change (see
    /// [`SceneWatcher::with_timing`]).
    pub fn new() -> SceneWatcher {
        SceneWatcher::with_timing(POLL_INTERVAL_MS, DEBOUNCE_MS)
    }

    /// Look every `interval_ms`; reload when the files stayed quiet for
    /// `debounce_ms` after the last change **or one poll interval, whichever is
    /// longer**. A quiet period shorter than the interval would end before the
    /// next look could show that the writer is not done (the reload would
    /// fire, with a half-written file, and the second write step would come as
    /// a second reload at best).
    pub fn with_timing(interval_ms: u64, debounce_ms: u64) -> SceneWatcher {
        SceneWatcher {
            poller: Poller::new(interval_ms),
            debounce: Debounce::new(debounce_ms.max(interval_ms)),
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
        assert!(!w.tick(1400), "a whole poll interval of quiet is needed");
        assert!(!w.tick(1499));
        assert!(w.tick(1500), "the look at 1500 found nothing new");
        assert!(!w.tick(1600));
        assert!(!w.tick(2000), "nothing new, nothing to do");
    }

    /// The case the debounce is for, at the default timing (look every 250 ms,
    /// quiet period 150 ms raised to one interval): the editor truncates and
    /// writes a first part, a look sees it, the rest is written 50 ms later.
    /// Exactly one reload must come, after the second step, and the file must
    /// be whole when it is read.
    #[test]
    fn a_two_step_write_at_the_default_timing_is_one_reload_after_the_second_step() {
        for second_step_at in [260_u64, 330, 499] {
            let dir = TempDir::new("defaults");
            let f = dir.0.join("scene.ron");
            std::fs::write(&f, "old content").unwrap();
            let mut w = SceneWatcher::new();
            w.set_paths([f.clone()]);
            assert!(!w.tick(0), "baseline");
            std::fs::write(&f, "half").unwrap(); // step 1, before the look at 250
            let mut reloads = Vec::new();
            for now in (10..=2000).step_by(10) {
                if now == second_step_at - second_step_at % 10 {
                    std::fs::write(&f, "the whole new content").unwrap(); // step 2
                }
                if w.tick(now) {
                    reloads.push((now, std::fs::read_to_string(&f).unwrap()));
                }
            }
            assert_eq!(
                reloads.len(),
                1,
                "second step at {second_step_at}: {reloads:?}"
            );
            let (at, content) = &reloads[0];
            assert_eq!(content, "the whole new content", "read after the last step");
            assert!(*at > second_step_at, "after the second step: {at}");
            // And the look after the second step was the last one with news.
            assert!(*at >= 500, "{at}");
        }
    }

    #[test]
    fn the_debounce_is_never_shorter_than_one_poll_interval() {
        let dir = TempDir::new("quiet");
        let f = dir.0.join("a");
        std::fs::write(&f, "1").unwrap();
        // Poll every 400 ms, "debounce" 100 ms: still waits a whole interval.
        let mut w = SceneWatcher::with_timing(400, 100);
        w.set_paths([f.clone()]);
        assert!(!w.tick(0));
        std::fs::write(&f, "22").unwrap();
        assert!(!w.tick(400), "seen");
        assert!(!w.tick(500), "100 ms later is too early");
        assert!(!w.tick(799));
        assert!(w.tick(800));
    }

    #[test]
    fn untouched_files_cost_a_stat_not_a_read() {
        let dir = TempDir::new("cost");
        let (old, fresh) = (dir.0.join("old.png"), dir.0.join("fresh.png"));
        std::fs::write(&old, "old picture").unwrap();
        std::fs::write(&fresh, "fresh picture").unwrap();
        let an_hour_ago = SystemTime::now() - std::time::Duration::from_secs(3600);
        let f = std::fs::File::options().write(true).open(&old).unwrap();
        f.set_modified(an_hour_ago).unwrap();
        drop(f);
        let mut p = Poller::new(1);
        p.set_paths([old.clone(), fresh.clone()]);
        assert_eq!(p.hashes_computed(), 2, "one baseline hash each");
        for now in 0..10 {
            assert!(!p.poll(now));
        }
        assert_eq!(
            p.hashes_computed(),
            2 + 10,
            "only the recently written file is hashed again, at every look"
        );
        // A real change of the old file is still seen (size and time differ),
        // and hashed once to remember it.
        std::fs::write(&old, "a different old picture").unwrap();
        assert!(p.poll(100));
        assert_eq!(p.hashes_computed(), 2 + 10 + 1 + 1);
    }

    #[test]
    fn a_file_is_only_hashed_again_while_its_time_is_recent() {
        let now = SystemTime::now();
        let ns = |t: SystemTime| t.duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let sec = std::time::Duration::from_secs;
        assert!(recent(Some(ns(now)), now));
        assert!(recent(Some(ns(now - sec(1))), now));
        assert!(!recent(Some(ns(now - sec(3))), now));
        assert!(recent(Some(ns(now + sec(60))), now), "a time in the future");
        assert!(recent(None, now), "no time: cannot be trusted");
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
        std::fs::write(&f, "v2").unwrap(); // back before the next look
        assert!(!w.tick(500), "the next look sees it: settling again");
        assert!(w.tick(750), "reload: the file is read as v2");
        assert!(!w.tick(1000), "the next look finds nothing new");
        assert!(!w.tick(1500));
        std::fs::write(&f, "v3").unwrap();
        assert!(!w.tick(1750));
        assert!(w.tick(2000), "a real later change still reloads");
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
