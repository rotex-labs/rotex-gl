//! GLES 3.0 / WebGL2 translation. Absent features return [`Error::unsupported`].

use std::collections::HashMap;

use glow::HasContext;
use rotex_core::{Error, GpuBackend};
use rotex_types::{
    BackendCapabilities, BindGroupId, BindGroupLayoutId, BufferId, BufferUsage, BufferUsages,
    ComputePipelineId, CreatedResources, Extent2D, GraphicsPipelineId, IndexFormat, MaterialId,
    MeshId, PassColorTarget, QuerySetId, ResourceBatchCreate, ResourceBatchUpdate,
    ResourceCreateDescriptor, ResourceHandle, ResourceUpdateDescriptor, RhiCommand, SamplerId,
    ShaderPayload, SurfaceDescriptor, TextureFormat, TextureId, TextureReadback, VertexFormat,
    VertexStepMode,
};

use crate::context::GlContext;

mod execute;
mod program;
mod resources;

struct GpuBuffer {
    buffer: glow::Buffer,
    bytes: Vec<u8>,
}

struct GpuTexture {
    texture: glow::Texture,
    framebuffer: Option<glow::Framebuffer>,
    width: u32,
    height: u32,
    format: TextureFormat,
}

struct GpuPipeline {
    program: glow::Program,
    topology: u32,
    layouts: Vec<rotex_types::VertexBufferLayout>,
}

struct GpuMesh {
    vertex: Option<glow::Buffer>,
    index: Option<glow::Buffer>,
}

struct BoundGroup {
    buffers: Vec<(u32, BufferId, u64, u64)>,
}

/// Translates [`rotex_types::RhiCommand`] values to GLES 3.0 or WebGL2.
pub struct GlBridge {
    context: GlContext,
    capabilities: BackendCapabilities,
    buffers: HashMap<BufferId, GpuBuffer>,
    textures: HashMap<TextureId, GpuTexture>,
    pipelines: HashMap<GraphicsPipelineId, GpuPipeline>,
    materials: HashMap<MaterialId, glow::Program>,
    meshes: HashMap<MeshId, GpuMesh>,
    samplers: HashMap<SamplerId, glow::Sampler>,
    layouts: HashMap<BindGroupLayoutId, ()>,
    groups: HashMap<BindGroupId, BoundGroup>,
    queries: HashMap<QuerySetId, ()>,
    compute: HashMap<ComputePipelineId, ()>,
    shaders: HashMap<rotex_types::ShaderId, rotex_types::ShaderArtifact>,
    bound_pipeline: Option<GraphicsPipelineId>,
    vertex_bindings: HashMap<u32, (BufferId, u64)>,
    index_binding: Option<(BufferId, u64, IndexFormat)>,
    draw_framebuffer: Option<glow::Framebuffer>,
}

impl GlBridge {
    pub(crate) fn new(context: GlContext, capabilities: BackendCapabilities) -> Self {
        Self {
            context,
            capabilities,
            buffers: HashMap::new(),
            textures: HashMap::new(),
            pipelines: HashMap::new(),
            materials: HashMap::new(),
            meshes: HashMap::new(),
            samplers: HashMap::new(),
            layouts: HashMap::new(),
            groups: HashMap::new(),
            queries: HashMap::new(),
            compute: HashMap::new(),
            shaders: HashMap::new(),
            bound_pipeline: None,
            vertex_bindings: HashMap::new(),
            index_binding: None,
            draw_framebuffer: None,
        }
    }

    fn absent(what: &'static str) -> Error {
        Error::unsupported(what)
    }
}

impl GpuBackend for GlBridge {
    fn capabilities(&self) -> BackendCapabilities {
        self.capabilities
    }

    fn attach_surface(&mut self, _surface_descriptor: SurfaceDescriptor) -> Result<(), Error> {
        self.context.with_gl(|_| ());
        Ok(())
    }

    fn create_resources(
        &mut self,
        descriptor: ResourceBatchCreate,
    ) -> Result<CreatedResources, Error> {
        let mut handles = Vec::new();
        for resource in descriptor.resources {
            handles.push(self.create_one(resource)?);
        }
        Ok(CreatedResources { handles })
    }

    fn update_resources(&mut self, descriptor: ResourceBatchUpdate) -> Result<(), Error> {
        for update in descriptor.updates {
            self.update_one(update)?;
        }
        Ok(())
    }

    fn execute(&mut self, commands: &[RhiCommand]) -> Result<(), Error> {
        for command in commands {
            self.execute_one(command)?;
        }
        Ok(())
    }

    fn resize(&mut self, extent: Extent2D) -> Result<(), Error> {
        let extent = extent.clamped();
        self.context.with_gl(|gl| unsafe {
            gl.viewport(0, 0, extent.width as i32, extent.height as i32);
        });
        Ok(())
    }

    fn read_texture(&mut self, id: TextureId) -> Result<TextureReadback, Error> {
        let texture = self
            .textures
            .get(&id)
            .ok_or_else(|| Error::unsupported("texture not found"))?;
        let width = texture.width;
        let height = texture.height;
        let format = texture.format;
        let framebuffer = texture.framebuffer;
        let mut data = vec![0u8; (width * height * 4) as usize];
        self.context.with_gl(|gl| unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, framebuffer);
            gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut data)),
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        });
        Ok(TextureReadback {
            data,
            format,
            width,
            height,
        })
    }

    fn destroy(self) {
        let bridge = self;
        let buffers: Vec<_> = bridge
            .buffers
            .values()
            .map(|buffer| buffer.buffer)
            .collect();
        let textures: Vec<_> = bridge
            .textures
            .values()
            .map(|texture| (texture.texture, texture.framebuffer))
            .collect();
        let programs: Vec<_> = bridge
            .pipelines
            .values()
            .map(|pipeline| pipeline.program)
            .chain(bridge.materials.values().copied())
            .collect();
        let meshes: Vec<_> = bridge
            .meshes
            .values()
            .flat_map(|mesh| [mesh.vertex, mesh.index])
            .flatten()
            .collect();
        let samplers: Vec<_> = bridge.samplers.values().copied().collect();
        bridge.context.with_gl(|gl| unsafe {
            for buffer in buffers.into_iter().chain(meshes) {
                gl.delete_buffer(buffer);
            }
            for (texture, framebuffer) in textures {
                if let Some(framebuffer) = framebuffer {
                    gl.delete_framebuffer(framebuffer);
                }
                gl.delete_texture(texture);
            }
            for program in programs {
                gl.delete_program(program);
            }
            for sampler in samplers {
                gl.delete_sampler(sampler);
            }
        });
    }
}
