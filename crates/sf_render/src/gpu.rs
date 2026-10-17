//! wgpu instance, adapter and device setup, with or without a window surface.

use std::fmt;
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

/// Errors from setting up or driving the renderer.
#[derive(Debug)]
pub enum RenderError {
    /// No adapter satisfied the request (e.g. no Vulkan driver installed).
    NoAdapter(String),
    Device(String),
    Surface(String),
    /// Reading pixels back from the GPU failed.
    Readback(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter(e) => write!(f, "no suitable GPU adapter: {e}"),
            Self::Device(e) => write!(f, "cannot create GPU device: {e}"),
            Self::Surface(e) => write!(f, "surface error: {e}"),
            Self::Readback(e) => write!(f, "pixel readback failed: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// How to pick the adapter.
#[derive(Clone, Copy, Debug)]
pub struct GpuOptions {
    pub backends: wgpu::Backends,
    pub power_preference: wgpu::PowerPreference,
    /// Only accept a software ("fallback") adapter: Mesa lavapipe on Linux,
    /// DX12 WARP on Windows.
    pub force_fallback: bool,
    /// How frames reach the window (ignored without one).
    pub present: Present,
    /// Let a window frame be copied out for screenshots
    /// ([`crate::Renderer::capture_next_frame`]). Off by default: the extra
    /// surface usage can change how a driver allocates the swap chain, so a
    /// benchmark leaves it off and measures the normal surface (ignored
    /// without a window; the offscreen target can always be captured).
    pub capture: bool,
}

/// How a window presents its frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Present {
    /// Wait for the display refresh (`Fifo`): no tearing, frame rate capped
    /// at the refresh rate. What the game uses.
    #[default]
    Vsync,
    /// Do not wait: `Immediate`, else `Mailbox`, else (with a note) `Fifo`.
    /// What the benchmark uses, so that frame times measure the work and not
    /// the display (progress.md follow-up 13).
    Uncapped,
}

impl Present {
    /// The present mode to configure from what the surface supports, and a
    /// note when the request could not be met.
    pub fn choose(self, supported: &[wgpu::PresentMode]) -> (wgpu::PresentMode, Option<String>) {
        use wgpu::PresentMode as M;
        match self {
            Present::Vsync => (M::Fifo, None),
            Present::Uncapped => {
                let pick = [M::Immediate, M::Mailbox]
                    .into_iter()
                    .find(|m| supported.contains(m));
                match pick {
                    Some(m) => (m, None),
                    None => (
                        M::Fifo,
                        Some(format!(
                            "the surface supports neither Immediate nor Mailbox ({supported:?}); \
                             vsync stays on and frame times are capped by the display"
                        )),
                    ),
                }
            }
        }
    }
}

impl Default for GpuOptions {
    fn default() -> Self {
        Self {
            backends: wgpu::Backends::all(),
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback: false,
            present: Present::Vsync,
            capture: false,
        }
    }
}

impl GpuOptions {
    /// What the game window uses: DX12 on Windows, the native APIs elsewhere
    /// (Vulkan, Metal; never GL), the discrete GPU first. `WGPU_BACKEND`
    /// (e.g. `dx12`, `vulkan`) and `WGPU_POWER_PREF` (`high`, `low`) override
    /// both, so a driver that hangs on one API can be worked around without a
    /// rebuild.
    ///
    /// Windows uses DX12 only because Vulkan hung during adapter creation on
    /// the test laptop (NVIDIA GTX 1650 with Optimus, 2026-10-15), while DX12
    /// started at once. DX12 is also what the CI screenshot tests use (WARP).
    pub fn window() -> Self {
        let native = if cfg!(windows) {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::PRIMARY
        };
        Self {
            backends: native.with_env(),
            power_preference: wgpu::PowerPreference::from_env()
                .unwrap_or(wgpu::PowerPreference::HighPerformance),
            force_fallback: false,
            present: Present::Vsync,
            capture: false,
        }
    }

    /// What the screenshot tests use: software adapter first, the primary
    /// native APIs only (so a GL emulation never sneaks in).
    pub fn software() -> Self {
        Self {
            backends: wgpu::Backends::PRIMARY,
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback: true,
            present: Present::Vsync,
            capture: false,
        }
    }
}

/// Instance, adapter, device and queue.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The device can time whole frames (`TIMESTAMP_QUERY` and
    /// `TIMESTAMP_QUERY_INSIDE_ENCODERS`), see [`crate::gpu_timer`].
    pub timestamps: bool,
}

impl Gpu {
    /// Create an instance with the options' backends.
    pub fn instance(options: &GpuOptions) -> wgpu::Instance {
        trace(&format!("instance ({:?})", options.backends));
        wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: options.backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        })
    }

    /// Adapter + device, optionally constrained to present to `surface`.
    pub fn new(
        instance: wgpu::Instance,
        options: &GpuOptions,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, RenderError> {
        trace(&format!("request adapter ({:?})", options.power_preference));
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: options.power_preference,
            force_fallback_adapter: options.force_fallback,
            compatible_surface: surface,
            ..Default::default()
        }))
        .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        let info = adapter.get_info();
        trace(&format!(
            "adapter: {} [{:?}, {:?}]",
            info.name, info.device_type, info.backend
        ));

        // Frame timing is optional: ask for it only when both features exist.
        let timing =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        let timestamps = adapter.features().contains(timing);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("sf_render device"),
            required_features: if timestamps {
                timing
            } else {
                wgpu::Features::empty()
            },
            ..Default::default()
        }))
        .map_err(|e| RenderError::Device(e.to_string()))?;
        trace("device ready");

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            timestamps,
        })
    }

    /// Headless device (no surface).
    pub fn headless(options: &GpuOptions) -> Result<Self, RenderError> {
        Self::new(Self::instance(options), options, None)
    }

    /// Human-readable adapter description, e.g.
    /// `llvmpipe (LLVM 20.1.2, 256 bits) [Cpu, Vulkan]`.
    pub fn adapter_summary(&self) -> String {
        let info = self.adapter.get_info();
        format!("{} [{:?}, {:?}]", info.name, info.device_type, info.backend)
    }
}

/// How long [`HeadlessSlot::acquire`] waits for another headless device
/// before it gives up (a device held by the waiting thread itself would
/// otherwise wait forever).
const SLOT_WAIT: Duration = Duration::from_secs(20 * 60);

/// `true` while a headless renderer of this process exists.
static HEADLESS_SLOT: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

/// The right to have a headless (offscreen) device: only one exists per
/// process at a time, so a test binary whose tests run in parallel threads
/// never has two software devices creating or drawing at once.
///
/// Why: on Windows the DX12 software adapter (WARP) crashed with
/// STATUS_ACCESS_VIOLATION (0xc0000005) when several screenshot tests built
/// and used a device at the same moment (PR #18, run 39009873035, twice on the
/// same commit). CI used to work around it with `--test-threads=1` for all
/// tests; this lock serialises only the GPU tests (the other tests of the
/// binary still run in parallel) and sits where every offscreen renderer is
/// made, so no test helper can forget it.
///
/// [`Renderer::headless`](crate::Renderer::headless) takes the slot before it
/// creates the device and keeps it until the renderer is dropped, the last
/// thing that happens to it. Holding two headless renderers in one thread
/// would wait for the first one forever; the wait ends in a panic after
/// [`SLOT_WAIT`] instead of a hung test run. A window renderer takes no slot.
pub struct HeadlessSlot(());

impl HeadlessSlot {
    /// Wait for the slot and take it.
    ///
    /// # Panics
    /// After [`SLOT_WAIT`] (twenty minutes) without getting it.
    pub fn acquire() -> Self {
        let (lock, free) = &HEADLESS_SLOT;
        let mut taken = lock.lock().unwrap_or_else(PoisonError::into_inner);
        while *taken {
            let (guard, timeout) = free
                .wait_timeout(taken, SLOT_WAIT)
                .unwrap_or_else(PoisonError::into_inner);
            taken = guard;
            assert!(
                !(timeout.timed_out() && *taken),
                "waited {SLOT_WAIT:?} for the headless GPU slot: a headless renderer is held \
                 (by this thread?) and never dropped; only one can exist at a time"
            );
        }
        *taken = true;
        Self(())
    }
}

impl Drop for HeadlessSlot {
    fn drop(&mut self) {
        let (lock, free) = &HEADLESS_SLOT;
        *lock.lock().unwrap_or_else(PoisonError::into_inner) = false;
        free.notify_one();
    }
}

/// Startup progress on stderr when the environment variable `SF_GPU_TRACE` is
/// set: shows at which step a driver hangs (instance, adapter, device, surface).
pub(crate) fn trace(step: &str) {
    if std::env::var_os("SF_GPU_TRACE").is_some() {
        eprintln!("sf_render gpu: {step}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    /// However many threads ask, only one holds the slot at a time, and each
    /// gets it in the end.
    #[test]
    fn the_headless_slot_is_held_by_one_thread_at_a_time() {
        static HOLDERS: AtomicUsize = AtomicUsize::new(0);
        static MOST: AtomicUsize = AtomicUsize::new(0);
        static DONE: AtomicUsize = AtomicUsize::new(0);
        let threads: Vec<_> = (0..8)
            .map(|_| {
                thread::spawn(|| {
                    let _slot = HeadlessSlot::acquire();
                    let now = HOLDERS.fetch_add(1, Ordering::SeqCst) + 1;
                    MOST.fetch_max(now, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(10));
                    HOLDERS.fetch_sub(1, Ordering::SeqCst);
                    DONE.fetch_add(1, Ordering::SeqCst);
                })
            })
            .collect();
        for t in threads {
            t.join().expect("no thread panicked");
        }
        assert_eq!(MOST.load(Ordering::SeqCst), 1);
        assert_eq!(DONE.load(Ordering::SeqCst), 8);
    }

    /// A thread that panics while it holds the slot does not lock the others
    /// out: the slot's drop runs while unwinding.
    #[test]
    fn a_panic_while_holding_the_slot_releases_it() {
        let held = thread::spawn(|| {
            let _slot = HeadlessSlot::acquire();
            panic!("a failing screenshot test");
        })
        .join();
        assert!(held.is_err());
        drop(HeadlessSlot::acquire());
    }
}
