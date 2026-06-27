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

        // `request_adapter` returns `Result` in this wgpu generation —
        // confirmed against source, so `.map_err(...)` below is correct.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("no suitable GPU adapter: {}", e))?;

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
}
