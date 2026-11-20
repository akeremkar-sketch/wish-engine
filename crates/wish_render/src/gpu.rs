//! The GPU: adapter and device selection, and turning wgpu errors into
//! diagnostics.
//!
//! wgpu's default reaction to an error nobody caught is to panic. The engine
//! never panics in library code, so every [`Gpu`] installs a handler that
//! records such errors, and the work it does is wrapped in error scopes whose
//! errors come back as [`Diagnostic`]s.
//!
//! **One GPU at a time.** Measured in M3 WP3: on Windows' software adapter
//! (WARP), two devices drawing at the same time in one process crash it with
//! an access violation inside the driver, while threads sharing one device
//! are fine. The engine never crashes, so all GPU work in a process takes
//! one lock ([`gpu_lock`]). Normally a process has a single device anyway
//! ([`Gpu::shared`]), and then the lock costs nothing.
//!
//! **A frame is finished under that lock.** Measured in M12 (the render
//! foundation note, section 3.2): on WARP, making a bind group changes what
//! the live bind groups of the same layout read their uniform buffer from,
//! even for a frame that is still being executed. So a frame is submitted
//! with `Gpu::finish`, which waits for it before the lock is let go.
//!
//! **Every texture and buffer is created here** (`Gpu::create_texture`,
//! `Gpu::create_buffer`), so that [`Gpu::created`] counts them all and a test
//! can prove that a frame creates none. `tests/rules.rs` reads the source and
//! fails when another file creates one or submits commands on its own.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

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

/// How many textures and buffers have been created on a device since it was
/// opened, by everything that draws with it: render targets, uploaded meshes
/// and textures, per-frame buffers, readback buffers. Two frames of the same
/// scene and size create none, which is what tests and `wen bench --render`
/// read these counts for. (Bind groups, views and samplers are not counted.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Created {
    /// Textures.
    pub textures: u64,
    /// Buffers.
    pub buffers: u64,
}

/// A GPU device ready to draw.
pub struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    adapter: AdapterSummary,
    uncaptured: Arc<Mutex<Vec<String>>>,
    /// Why the device was lost, once it has been.
    lost: Arc<Mutex<Option<String>>>,
    /// Textures created so far.
    textures_created: AtomicU64,
    /// Buffers created so far.
    buffers_created: AtomicU64,
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

/// Serialises all GPU work in the process (see the module documentation).
/// A panic while holding it does not poison it for everyone else: the lock
/// guards no data, only the driver.
pub(crate) fn gpu_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The process's shared devices: one hardware, one software.
static SHARED: [OnceLock<std::result::Result<Arc<Gpu>, Diagnostic>>; 2] =
    [OnceLock::new(), OnceLock::new()];

impl Gpu {
    /// The process's device of this kind, opened on first use and shared
    /// after that. Prefer this to [`Gpu::new`]: one device per process is
    /// what the engine is built for, and opening a device takes time.
    pub fn shared(options: GpuOptions) -> Result<Arc<Self>> {
        SHARED[usize::from(options.software)]
            .get_or_init(|| Self::new(options).map(Arc::new))
            .clone()
    }

    /// Opens a GPU device. Fails with `no_gpu_adapter` when no suitable
    /// adapter exists, never by crashing.
    pub fn new(options: GpuOptions) -> Result<Self> {
        let _only_one = gpu_lock();
        // WGPU_VALIDATION, WGPU_DEBUG etc. turn wgpu's debugging aids on or off.
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
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
        let lost = Arc::new(Mutex::new(None));
        let lost_sink = Arc::clone(&lost);
        device.set_device_lost_callback(move |reason, message| {
            if let Ok(mut lost) = lost_sink.lock() {
                *lost = Some(format!("{reason:?}: {message}"));
            }
        });
        Ok(Self {
            device,
            queue,
            adapter: summary,
            uncaptured,
            lost,
            textures_created: AtomicU64::new(0),
            buffers_created: AtomicU64::new(0),
        })
    }

    /// Creates a texture. Every texture of the crate is created through
    /// this, so that [`Gpu::created`] counts it. Call inside
    /// [`Gpu::checked`], like all GPU work.
    pub(crate) fn create_texture(&self, descriptor: &wgpu::TextureDescriptor<'_>) -> wgpu::Texture {
        self.textures_created.fetch_add(1, Ordering::Relaxed);
        self.device.create_texture(descriptor)
    }

    /// Creates a buffer. Every buffer of the crate is created through this,
    /// so that [`Gpu::created`] counts it. The engine never maps a buffer at
    /// creation (that can panic): fill it through the queue. Call inside
    /// [`Gpu::checked`], like all GPU work.
    pub(crate) fn create_buffer(&self, descriptor: &wgpu::BufferDescriptor<'_>) -> wgpu::Buffer {
        self.buffers_created.fetch_add(1, Ordering::Relaxed);
        self.device.create_buffer(descriptor)
    }

    /// How many textures and buffers have been created on this device so
    /// far, by anything: for tests and measurements that prove a frame
    /// reuses what the frame before it made. A device shared between threads
    /// counts all of them together.
    #[doc(hidden)]
    pub fn created(&self) -> Created {
        Created {
            textures: self.textures_created.load(Ordering::Relaxed),
            buffers: self.buffers_created.load(Ordering::Relaxed),
        }
    }

    /// Fails with `gpu_lost` once the device has been lost: everything after
    /// that would fail in confusing ways, so say what really happened.
    pub(crate) fn check_lost(&self, what: &str) -> Result<()> {
        let lost = self.lost.lock().ok().and_then(|lost| lost.clone());
        match lost {
            Some(reason) => Err(gpu_error(
                "gpu_lost",
                format!("The GPU device was lost before it could {what} ({reason})."),
                "Restart the run. If it keeps happening, update the graphics driver or switch adapter (--software / hardware).",
            )),
            None => Ok(()),
        }
    }

    /// The adapter in use.
    pub fn adapter(&self) -> &AdapterSummary {
        &self.adapter
    }

    /// Runs `work` inside out-of-memory, internal and validation error
    /// scopes and returns what went wrong as a diagnostic. The root cause
    /// comes first: a buffer that could not be allocated also makes every
    /// later use of it a validation error, so memory and internal errors
    /// are reported before validation errors.
    pub(crate) fn checked<T>(&self, what: &str, work: impl FnOnce(&Self) -> T) -> Result<T> {
        let _only_one = gpu_lock();
        self.check_lost(what)?;
        let memory = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = self.device.push_error_scope(wgpu::ErrorFilter::Internal);
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let value = work(self);
        let validation = pollster::block_on(validation.pop());
        let internal = pollster::block_on(internal.pop());
        let memory = pollster::block_on(memory.pop());
        if let Some(error) = memory {
            return Err(gpu_error(
                "gpu_out_of_memory",
                format!("The GPU ran out of memory trying to {what}: {error}"),
                "Close other programs that use the GPU, draw a smaller image, or use --software.",
            ));
        }
        if let Some(error) = internal.or(validation) {
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

    /// Submits a frame's commands and waits until the GPU has executed them.
    /// Call it inside [`Gpu::checked`], as the last thing: the GPU lock is
    /// then held until the frame is done.
    ///
    /// A frame that draws through bind groups must be finished before
    /// another thread makes a bind group. On the software adapter of Windows
    /// a new bind group changes what every live group of the same layout
    /// reads its uniform buffer from, also for a command list that is still
    /// executing, and two renderers on one device share their layouts
    /// (measured: design note M12 render foundation, section 3.2). The
    /// picture is read back after this anyway, so the wait costs nothing.
    pub(crate) fn finish(&self, encoder: wgpu::CommandEncoder) -> Result<()> {
        self.queue.submit([encoder.finish()]);
        self.wait()
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
