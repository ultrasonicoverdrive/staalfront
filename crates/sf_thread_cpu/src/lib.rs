//! The CPU time of the calling thread (WP1.16a, the hitch log).
//!
//! `std` has no thread CPU time. On Windows [`thread_cpu_time`] reads it with `GetThreadTimes`; on
//! other platforms it returns `None` and [`MEASURED`] is false ("not measured"). A frame that was long
//! with little CPU time means the thread was not running (another program, the antivirus, a modal
//! window loop); a long frame with all of it means our code was busy.
//!
//! This is the only crate of the workspace that may contain unsafe code (the workspace forbids it):
//! one call into the Windows API, whose arguments are a pseudo handle and four out pointers to values
//! on the caller's stack.

use std::time::Duration;

/// Whether this platform measures the thread's CPU time.
pub const MEASURED: bool = cfg!(windows);

/// A Windows `FILETIME` span (counts of 100 ns, in two 32-bit halves) as a duration.
pub fn filetime_duration(low: u32, high: u32) -> Duration {
    let hundred_ns = (u64::from(high) << 32) | u64::from(low);
    Duration::new(
        hundred_ns / 10_000_000,
        ((hundred_ns % 10_000_000) * 100) as u32,
    )
}

/// The CPU time (kernel and user) the calling thread has used since it began; `None` where it is not
/// measured or the call failed.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn thread_cpu_time() -> Option<Duration> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: `GetCurrentThread` returns the pseudo handle of the calling thread, valid for the call;
    // the four out pointers point to live FILETIME values on this stack.
    let ok = unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    (ok != 0).then(|| {
        filetime_duration(kernel.dwLowDateTime, kernel.dwHighDateTime)
            + filetime_duration(user.dwLowDateTime, user.dwHighDateTime)
    })
}

/// Not measured off Windows.
#[cfg(not(windows))]
pub fn thread_cpu_time() -> Option<Duration> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filetime_is_a_count_of_hundred_nanoseconds() {
        assert_eq!(filetime_duration(0, 0), Duration::ZERO);
        assert_eq!(filetime_duration(10_000_000, 0), Duration::from_secs(1));
        assert_eq!(filetime_duration(1, 0), Duration::from_nanos(100));
        // The high half counts 2^32 units: 429.4967296 s.
        assert_eq!(
            filetime_duration(0, 1),
            Duration::from_nanos(429_496_729_600)
        );
    }

    /// On Windows the clock runs forward while the thread works; elsewhere it says it is not measured.
    #[test]
    fn the_clock_is_measured_on_windows_only() {
        let first = thread_cpu_time();
        assert_eq!(first.is_some(), MEASURED);
        if let Some(first) = first {
            let mut x = 0u64;
            while thread_cpu_time().unwrap() < first + Duration::from_millis(30) {
                x = x.wrapping_add(1);
                std::hint::black_box(x);
            }
        }
    }
}
