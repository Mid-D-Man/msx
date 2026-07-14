// render/msx-render-gpu/src/context.rs
//! Instance → Adapter → Device → Queue setup, the one-time GPU handshake
//! every render needs. No `Surface` anywhere in this crate — it renders
//! off-screen into a `Texture` and reads the result back to CPU memory
//! (see `target.rs`); a window-backed `Surface` is `apps/msx-viewer`'s
//! concern.

pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl GpuContext {
    /// Blocks on the async instance → adapter → device handshake via
    /// `pollster` — wgpu's setup calls are async because browsers need
    /// that; a CLI/offscreen renderer just wants the result synchronously.
    pub fn new() -> Result<Self, String> {
        pollster::block_on(Self::new_async())
    }

    async fn new_async() -> Result<Self, String> {
        // NOTE: verified directly against wgpu 26.0.1's source
        // (wgpu-types/src/instance.rs) — `InstanceDescriptor` has exactly
        // `backends` / `flags` / `memory_budget_thresholds` /
        // `backend_options`, no `display` field, and `Instance::new` takes
        // `&InstanceDescriptor`, not an owned one.
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: Default::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
        });

        let adapter = Self::request_adapter_with_fallback(&instance).await?;

        // NOTE: same verification — `DeviceDescriptor` in 26.0.1 has
        // `label` / `required_features` / `required_limits` /
        // `memory_hints` / `trace`. No `experimental_features` field exists
        // on this version (it's from a later API generation than what was
        // available when this was first written).
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("msx-render-gpu device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("failed to request GPU device: {}", e))?;

        Ok(GpuContext { device, queue })
    }

    /// Two-stage adapter request, not a single one — different platforms
    /// (and CI runners in particular) fail to expose a real GPU adapter
    /// for different reasons, and there's no OS-specific branching this
    /// crate needs to write to handle it, because wgpu's own
    /// `force_fallback_adapter` flag already abstracts the actual
    /// mechanism (Linux: Mesa's llvmpipe/lavapipe software rasterizer;
    /// macOS/Windows: their own software paths) per platform:
    ///
    /// 1. First, ask for a real, hardware-backed adapter
    ///    (`force_fallback_adapter: false`) — this succeeds immediately
    ///    on essentially every real desktop/laptop, so the common case
    ///    pays no extra cost.
    /// 2. Only if that fails, retry once with `force_fallback_adapter:
    ///    true` — this is what lets a headless CI runner or minimal
    ///    container with no real GPU still produce a render (much
    ///    slower, CPU-emulated, but correct), rather than failing
    ///    outright the moment there's no physical GPU in the box.
    ///
    /// If *both* fail, the error message says so plainly rather than
    /// just surfacing wgpu's raw error text, since "no adapter at all,
    /// not even a software one" almost always means the machine is
    /// missing graphics drivers entirely — a fixable, diagnosable
    /// situation, not a code bug.
    async fn request_adapter_with_fallback(instance: &wgpu::Instance) -> Result<wgpu::Adapter, String> {
        let real = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await;
        if let Ok(adapter) = real {
            return Ok(adapter);
        }

        instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter: true,
            })
            .await
            .map_err(|e| {
                format!(
                    "no GPU adapter available, not even a software fallback ({e}). This usually means the \
                     machine has no graphics drivers installed at all — common on headless CI runners and \
                     minimal containers. On Linux, `apt-get install -y mesa-vulkan-drivers libgl1-mesa-dri` \
                     (or your distro's equivalent) is the usual fix; on macOS/Windows this is unusual outside \
                     of a stripped-down VM/container image."
                )
            })
    }
}
