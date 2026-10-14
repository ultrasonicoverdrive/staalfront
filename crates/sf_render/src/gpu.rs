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
}

impl Default for GpuOptions {
    fn default() -> Self {
        Self {
            backends: wgpu::Backends::all(),
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback: false,
        }
    }
}

impl GpuOptions {
    /// What the screenshot tests use: software adapter first, the primary
    /// native APIs only (so a GL emulation never sneaks in).
    pub fn software() -> Self {
        Self {
            backends: wgpu::Backends::PRIMARY,
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback: true,
        }
    }
}

/// Instance, adapter, device and queue.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Create an instance with the options' backends.
    pub fn instance(options: &GpuOptions) -> wgpu::Instance {
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
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: options.power_preference,
            force_fallback_adapter: options.force_fallback,
            compatible_surface: surface,
            ..Default::default()
        }))
        .map_err(|e| RenderError::NoAdapter(e.to_string()))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("sf_render device"),
            ..Default::default()
        }))
        .map_err(|e| RenderError::Device(e.to_string()))?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
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
