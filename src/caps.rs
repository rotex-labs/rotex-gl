use rotex_core::StaticCaps;
use rotex_types::{BackendCapabilities, CoreCaps, FeatureSet, Limits, Support};

use crate::recorder::GlBridge;

pub(crate) const CAPS: CoreCaps = CoreCaps {
    compute: Support::Absent,
    storage_buffers: Support::Absent,
    push_constants: Support::Absent,
    barriers: Support::Emulated,
    queries: Support::Absent,
    copies: Support::Native,
    readback: Support::Native,
    render_to_texture: Support::Native,
    dynamic_offsets: Support::Native,
    explicit_pipelines: Support::Native,
};

impl StaticCaps for GlBridge {
    const CAPS: CoreCaps = CAPS;
}

pub(crate) fn capabilities() -> BackendCapabilities {
    BackendCapabilities {
        core: CAPS,
        limits: Limits::CORE_MIN,
        extensions: FeatureSet::empty(),
    }
}
