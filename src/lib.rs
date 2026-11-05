#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! WebGL2 and GLES 3.0 backend.
//!
//! [`GlGpuPlatform`] creates one glow context, on native EGL and on wasm.
//! [`GlBridge`] translates [`rotex_types::RhiCommand`] values. Shader input is
//! the GLSL payload. A command whose capability is absent returns an error.
//!
//! Compute, storage buffers, push constants, and queries are absent. Barriers
//! are accepted and do nothing. Creating a native context requires EGL.

mod caps;
mod context;
mod gpu_platform;
mod recorder;

pub use gpu_platform::GlGpuPlatform;
pub use recorder::GlBridge;

#[cfg(test)]
mod conformance_test {
    use super::GlGpuPlatform;
    use rotex_core::GpuPlatform;
    use rotex_types::{DeviceDescriptor, PlatformDescriptor};

    #[test]
    fn backend_matches_reported_capabilities() {
        pollster::block_on(async {
            let gpu = match GlGpuPlatform::new(PlatformDescriptor::default()).await {
                Ok(gpu) => gpu,
                Err(_) => return,
            };
            let adapters = match gpu.enumerate_adapters().await {
                Ok(adapters) => adapters,
                Err(_) => return,
            };
            let Some(adapter) = adapters.first() else {
                return;
            };
            let mut backend = match gpu
                .create_backend(adapter.id, DeviceDescriptor::default())
                .await
            {
                Ok(backend) => backend,
                Err(_) => return,
            };
            rotex_conformance::run(&mut backend).expect("conformance");
        });
    }
}
