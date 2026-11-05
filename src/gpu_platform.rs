use async_trait::async_trait;
use rotex_core::{Error, GpuPlatform};
use rotex_types::{
    AdapterCapabilities, AdapterId, AdapterInfo, AdapterQueueSupport, DeviceDescriptor,
    GpuApiBackend, HardwareFamily, Limits, PlatformDescriptor,
};

use crate::caps::{self, CAPS};
use crate::context::GlContext;
use crate::recorder::GlBridge;

/// GL implementation of [`GpuPlatform`](rotex_core::GpuPlatform).
///
/// One context is created up front and moved into [`GlBridge`](crate::GlBridge)
/// when the backend is created.
pub struct GlGpuPlatform {
    context: Option<GlContext>,
}

#[async_trait]
impl GpuPlatform for GlGpuPlatform {
    type Backend = GlBridge;

    async fn new(_platform_descriptor: PlatformDescriptor) -> Result<Self, Error> {
        Ok(Self {
            context: Some(GlContext::new()?),
        })
    }

    async fn enumerate_adapters(&self) -> Result<Vec<AdapterInfo>, Error> {
        if self.context.is_none() {
            return Err(Error::unsupported("gl context was already taken"));
        }
        let api = if cfg!(target_arch = "wasm32") {
            GpuApiBackend::WebGl
        } else {
            GpuApiBackend::OpenGl
        };
        Ok(vec![AdapterInfo {
            id: AdapterId(0),
            name: "WebGL2".to_string(),
            hardware_family: HardwareFamily::Other,
            vendor_id: 0,
            device_id: 0,
            driver_name: "gles".to_string(),
            driver_info: "3.0".to_string(),
            api_backend: api,
            capabilities: AdapterCapabilities {
                supports_window_presentation: true,
                queue_support: AdapterQueueSupport {
                    graphics_queue_count: 1,
                    compute_queue_count: 0,
                    transfer_queue_count: 1,
                },
                available_features: rotex_types::DeviceFeatures::default(),
                limits: Limits::CORE_MIN,
                extensions: rotex_types::FeatureSet::empty(),
                core: CAPS,
            },
        }])
    }

    async fn create_backend(
        mut self,
        adapter_id: AdapterId,
        _device_descriptor: DeviceDescriptor,
    ) -> Result<Self::Backend, Error> {
        if adapter_id.0 != 0 {
            return Err(Error::unsupported("gl adapter id is out of range"));
        }
        let context = self
            .context
            .take()
            .ok_or_else(|| Error::unsupported("gl context was already taken"))?;
        Ok(GlBridge::new(context, caps::capabilities()))
    }
}
