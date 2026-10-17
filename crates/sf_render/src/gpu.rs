//! wgpu instance, adapter and device setup, with or without a window surface.

use std::fmt;

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

/// Startup progress on stderr when the environment variable `SF_GPU_TRACE` is
/// set: shows at which step a driver hangs (instance, adapter, device, surface).
pub(crate) fn trace(step: &str) {
    if std::env::var_os("SF_GPU_TRACE").is_some() {
        eprintln!("sf_render gpu: {step}");
    }
}
