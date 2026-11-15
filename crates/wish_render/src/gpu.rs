//! The GPU: adapter and device selection, and turning wgpu errors into
//! diagnostics.
//!
//! wgpu's default reaction to an error nobody caught is to panic. The engine
//! never panics in library code, so every [`Gpu`] installs a handler that
//! records such errors, and the work it does is wrapped in error scopes whose
//! errors come back as [`Diagnostic`]s.

use std::sync::{Arc, Mutex};

use wish_core::{Diagnostic, Result};

/// How to choose the GPU.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuOptions {
    /// Use the software adapter (WARP on Windows, lavapipe or SwiftShader
    /// with Vulkan elsewhere) instead of real hardware. Results are the same
    /// on every machine with the same software adapter, which is what
    /// golden-image tests need.
    pub software: bool,
}

/// What kind of adapter is in use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterSummary {
    /// The adapter's name as the driver reports it.
    pub name: String,
    /// The graphics API in use (`"Vulkan"`, `"Dx12"`, `"Metal"`, …).
    pub backend: String,
    /// Whether it is a software (CPU) adapter.
    pub software: bool,
}

/// A GPU device ready to draw.
pub struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    adapter: AdapterSummary,
    uncaptured: Arc<Mutex<Vec<String>>>,
}

impl std::fmt::Debug for Gpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gpu")
            .field("adapter", &self.adapter)
            .finish_non_exhaustive()
    }
}

fn gpu_error(code: &'static str, message: String, hint: &str) -> Diagnostic {
    Diagnostic::error(code, message, hint)
}

impl Gpu {
    /// Opens a GPU device. Fails with `no_gpu_adapter` when no suitable
    /// adapter exists, never by crashing.
    pub fn new(options: GpuOptions) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: options.software,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .map_err(|error| {
            gpu_error(
                "no_gpu_adapter",
                format!(
                    "No {} graphics adapter is available: {error}.",
                    if options.software { "software" } else { "hardware" }
                ),
                if options.software {
                    "Install a software rasterizer: on Windows WARP comes with the system; on Linux install Mesa's lavapipe (package mesa-vulkan-drivers)."
                } else {
                    "Update the graphics driver, or use the software adapter (--software)."
                },
            )
        })?;
        let info = adapter.get_info();
        let summary = AdapterSummary {
            name: info.name.clone(),
            backend: format!("{:?}", info.backend),
            software: info.device_type == wgpu::DeviceType::Cpu,
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("wish"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| {
            gpu_error(
                "gpu_device_failed",
                format!(
                    "The adapter '{}' could not open a device: {error}.",
                    summary.name
                ),
                "Update the graphics driver, or try the software adapter (--software).",
            )
        })?;
        let uncaptured = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&uncaptured);
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            if let Ok(mut errors) = sink.lock() {
                errors.push(error.to_string());
            }
        }));
        Ok(Self {
            device,
            queue,
            adapter: summary,
            uncaptured,
        })
    }

    /// The adapter in use.
    pub fn adapter(&self) -> &AdapterSummary {
        &self.adapter
    }

    /// Runs `work` inside a validation and out-of-memory error scope and
    /// returns the first GPU error as a diagnostic.
    pub(crate) fn checked<T>(&self, what: &str, work: impl FnOnce(&Self) -> T) -> Result<T> {
        let memory = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let value = work(self);
        let validation = pollster::block_on(validation.pop());
        let memory = pollster::block_on(memory.pop());
        if let Some(error) = validation.or(memory) {
            return Err(gpu_error(
                "gpu_error",
                format!("The GPU refused to {what}: {error}"),
                "This is a bug in wish_render unless the message names your shader or data; please report it with the message.",
            ));
        }
        let leaked = self
            .uncaptured
            .lock()
            .map(|mut errors| std::mem::take(&mut *errors))
            .unwrap_or_default();
        if let Some(error) = leaked.first() {
            return Err(gpu_error(
                "gpu_error",
                format!("The GPU reported an error while trying to {what}: {error}"),
                "This is a bug in wish_render; please report it with the message.",
            ));
        }
        Ok(value)
    }

    /// Waits until the GPU has finished all submitted work.
    pub(crate) fn wait(&self) -> Result<()> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map(|_| ())
            .map_err(|error| {
                gpu_error(
                    "gpu_lost",
                    format!("Waiting for the GPU failed: {error}."),
                    "The device may have been lost (driver reset). Restart the run; if it repeats, update the driver or use --software.",
                )
            })
    }
}
