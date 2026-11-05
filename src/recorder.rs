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

    fn destroy(self: Box<Self>) {
        let bridge = *self;
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

impl GlBridge {
    fn create_one(&mut self, resource: ResourceCreateDescriptor) -> Result<ResourceHandle, Error> {
        match resource {
            ResourceCreateDescriptor::Buffer { id, buffer } => {
                let id = BufferId(id);
                ensure_absent(&self.buffers, &id)?;
                if buffer.usage == BufferUsage::Storage
                    || buffer.usages.contains(BufferUsages::STORAGE)
                {
                    return Err(Self::absent("storage buffers are absent on WebGL2"));
                }
                let bytes = buffer
                    .initial_data
                    .unwrap_or_else(|| vec![0; buffer.size as usize]);
                let gl_buffer = self.context.with_gl(|gl| unsafe {
                    let handle = gl
                        .create_buffer()
                        .map_err(|_| Error::unsupported("gl buffer allocation failed"))?;
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(handle));
                    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::DYNAMIC_DRAW);
                    Ok::<_, Error>(handle)
                })?;
                self.buffers.insert(
                    id,
                    GpuBuffer {
                        buffer: gl_buffer,
                        bytes,
                    },
                );
                Ok(ResourceHandle::Buffer(id))
            }
            ResourceCreateDescriptor::Texture { id, texture } => {
                let id = TextureId(id);
                ensure_absent(&self.textures, &id)?;
                let (gl_texture, framebuffer) = self.context.with_gl(|gl| unsafe {
                    let handle = gl
                        .create_texture()
                        .map_err(|_| Error::unsupported("gl texture allocation failed"))?;
                    gl.bind_texture(glow::TEXTURE_2D, Some(handle));
                    let pixels = if texture.data.is_empty() {
                        glow::PixelUnpackData::Slice(None)
                    } else {
                        glow::PixelUnpackData::Slice(Some(&texture.data))
                    };
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA8 as i32,
                        texture.width as i32,
                        texture.height as i32,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        pixels,
                    );
                    gl.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_MIN_FILTER,
                        glow::NEAREST as i32,
                    );
                    gl.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_MAG_FILTER,
                        glow::NEAREST as i32,
                    );
                    let framebuffer = if texture.render_attachment {
                        let framebuffer = gl
                            .create_framebuffer()
                            .map_err(|_| Error::unsupported("framebuffer allocation failed"))?;
                        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
                        gl.framebuffer_texture_2d(
                            glow::FRAMEBUFFER,
                            glow::COLOR_ATTACHMENT0,
                            glow::TEXTURE_2D,
                            Some(handle),
                            0,
                        );
                        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                        Some(framebuffer)
                    } else {
                        None
                    };
                    Ok::<_, Error>((handle, framebuffer))
                })?;
                self.textures.insert(
                    id,
                    GpuTexture {
                        texture: gl_texture,
                        framebuffer,
                        width: texture.width.max(1),
                        height: texture.height.max(1),
                        format: texture.format,
                    },
                );
                Ok(ResourceHandle::Texture(id))
            }
            ResourceCreateDescriptor::GraphicsPipeline { id, pipeline } => {
                let id = GraphicsPipelineId(id);
                ensure_absent(&self.pipelines, &id)?;
                let program =
                    self.link_program(&pipeline.shaders.vertex, &pipeline.shaders.fragment)?;
                let topology = match pipeline.topology {
                    rotex_types::PrimitiveTopology::PointList => glow::POINTS,
                    rotex_types::PrimitiveTopology::LineList => glow::LINES,
                    rotex_types::PrimitiveTopology::TriangleList => glow::TRIANGLES,
                    rotex_types::PrimitiveTopology::TriangleStrip => glow::TRIANGLE_STRIP,
                };
                self.pipelines.insert(
                    id,
                    GpuPipeline {
                        program,
                        topology,
                        layouts: pipeline.vertex_buffers,
                    },
                );
                Ok(ResourceHandle::GraphicsPipeline(id))
            }
            ResourceCreateDescriptor::Material { id, material } => {
                let id = MaterialId(id);
                ensure_absent(&self.materials, &id)?;
                let program =
                    self.link_program(&material.shaders.vertex, &material.shaders.fragment)?;
                self.materials.insert(id, program);
                Ok(ResourceHandle::Material(id))
            }
            ResourceCreateDescriptor::Mesh { id, mesh } => {
                let id = MeshId(id);
                ensure_absent(&self.meshes, &id)?;
                let vertex_bytes =
                    mesh.vertex_streams
                        .first()
                        .and_then(|stream| match &stream.data {
                            rotex_types::VertexStreamData::Static(bytes) => Some(bytes.clone()),
                            rotex_types::VertexStreamData::External(_) => None,
                        });
                let (vertex, index) = self.context.with_gl(|gl| unsafe {
                    let vertex = if let Some(bytes) = vertex_bytes.as_ref() {
                        let buffer = gl
                            .create_buffer()
                            .map_err(|_| Error::unsupported("mesh vertex buffer failed"))?;
                        gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
                        Some(buffer)
                    } else {
                        None
                    };
                    let index = if mesh.index_data.is_empty() {
                        None
                    } else {
                        let buffer = gl
                            .create_buffer()
                            .map_err(|_| Error::unsupported("mesh index buffer failed"))?;
                        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(buffer));
                        gl.buffer_data_u8_slice(
                            glow::ELEMENT_ARRAY_BUFFER,
                            &mesh.index_data,
                            glow::STATIC_DRAW,
                        );
                        Some(buffer)
                    };
                    Ok::<_, Error>((vertex, index))
                })?;
                self.meshes.insert(id, GpuMesh { vertex, index });
                Ok(ResourceHandle::Mesh(id))
            }
            ResourceCreateDescriptor::Sampler { id, sampler } => {
                let id = SamplerId(id);
                ensure_absent(&self.samplers, &id)?;
                let filter = match sampler.mag_filter {
                    rotex_types::FilterMode::Nearest => glow::NEAREST,
                    rotex_types::FilterMode::Linear => glow::LINEAR,
                };
                let wrap = match sampler.address_mode {
                    rotex_types::AddressMode::ClampToEdge => glow::CLAMP_TO_EDGE,
                    rotex_types::AddressMode::Repeat => glow::REPEAT,
                    rotex_types::AddressMode::MirrorRepeat => glow::MIRRORED_REPEAT,
                };
                let handle = self.context.with_gl(|gl| unsafe {
                    let handle = gl
                        .create_sampler()
                        .map_err(|_| Error::unsupported("sampler allocation failed"))?;
                    gl.sampler_parameter_i32(handle, glow::TEXTURE_MIN_FILTER, filter as i32);
                    gl.sampler_parameter_i32(handle, glow::TEXTURE_MAG_FILTER, filter as i32);
                    gl.sampler_parameter_i32(handle, glow::TEXTURE_WRAP_S, wrap as i32);
                    gl.sampler_parameter_i32(handle, glow::TEXTURE_WRAP_T, wrap as i32);
                    Ok::<_, Error>(handle)
                })?;
                self.samplers.insert(id, handle);
                Ok(ResourceHandle::Sampler(id))
            }
            ResourceCreateDescriptor::BindGroupLayout { id, layout } => {
                let id = BindGroupLayoutId(id);
                ensure_absent(&self.layouts, &id)?;
                if layout
                    .entries
                    .iter()
                    .any(|entry| entry.ty == rotex_types::BindingType::StorageBuffer)
                {
                    return Err(Self::absent("storage buffers are absent on WebGL2"));
                }
                self.layouts.insert(id, ());
                Ok(ResourceHandle::BindGroupLayout(id))
            }
            ResourceCreateDescriptor::BindGroup { id, group } => {
                let id = BindGroupId(id);
                ensure_absent(&self.groups, &id)?;
                if !self.layouts.contains_key(&group.layout) {
                    return Err(Error::unsupported("bind group layout not found"));
                }
                let mut buffers = Vec::new();
                for entry in group.entries {
                    if let rotex_types::BindGroupEntryDescriptor::Buffer {
                        binding,
                        buffer,
                        offset,
                        size,
                    } = entry
                    {
                        buffers.push((binding, buffer, offset, size));
                    }
                }
                self.groups.insert(id, BoundGroup { buffers });
                Ok(ResourceHandle::BindGroup(id))
            }
            ResourceCreateDescriptor::ComputePipeline { .. } => {
                Err(Self::absent("compute is absent on WebGL2"))
            }
            ResourceCreateDescriptor::QuerySet { .. } => {
                Err(Self::absent("queries are absent on WebGL2"))
            }
            ResourceCreateDescriptor::Shader { id, artifact } => {
                let id = rotex_types::ShaderId(id);
                ensure_absent(&self.shaders, &id)?;
                let es = artifact.payload(rotex_types::TargetId::GlslEs300);
                let desktop = artifact.payload(rotex_types::TargetId::GlslDesktop);
                #[cfg(target_arch = "wasm32")]
                let selected = es.or(desktop);
                #[cfg(not(target_arch = "wasm32"))]
                let selected = desktop.or(es);
                if selected.is_none() {
                    return Err(Error::unsupported("shader artifact has no GLSL payload"));
                }
                if let Some(cap) = artifact.unmet_capability(&[]) {
                    let message = match cap {
                        "compute" => "shader requires compute",
                        "storage_buffer" => "shader requires storage buffers",
                        _ => "shader requires an unsupported capability",
                    };
                    return Err(Error::unsupported(message));
                }
                self.shaders.insert(id, artifact);
                Ok(ResourceHandle::Shader(id))
            }
            _ => Err(Error::unsupported("resource is not implemented")),
        }
    }

    fn update_one(&mut self, update: ResourceUpdateDescriptor) -> Result<(), Error> {
        match update {
            ResourceUpdateDescriptor::Buffer { id, data } => {
                let buffer = self
                    .buffers
                    .get_mut(&id)
                    .ok_or_else(|| Error::unsupported("buffer not found"))?;
                let len = data.len();
                if buffer.bytes.len() < len {
                    buffer.bytes.resize(len, 0);
                }
                buffer.bytes[..len].copy_from_slice(&data);
                let gl_buffer = buffer.buffer;
                self.context.with_gl(|gl| unsafe {
                    gl.bind_buffer(glow::COPY_WRITE_BUFFER, Some(gl_buffer));
                    gl.buffer_sub_data_u8_slice(glow::COPY_WRITE_BUFFER, 0, &data);
                });
                Ok(())
            }
            ResourceUpdateDescriptor::Destroy { handle } => self.destroy_handle(handle),
            _ => Ok(()),
        }
    }

    fn destroy_handle(&mut self, handle: ResourceHandle) -> Result<(), Error> {
        match handle {
            ResourceHandle::Buffer(id) => {
                if let Some(buffer) = self.buffers.remove(&id) {
                    self.context
                        .with_gl(|gl| unsafe { gl.delete_buffer(buffer.buffer) });
                }
            }
            ResourceHandle::Texture(id) => {
                if let Some(texture) = self.textures.remove(&id) {
                    self.context.with_gl(|gl| unsafe {
                        if let Some(framebuffer) = texture.framebuffer {
                            gl.delete_framebuffer(framebuffer);
                        }
                        gl.delete_texture(texture.texture);
                    });
                }
            }
            ResourceHandle::GraphicsPipeline(id) => {
                if let Some(pipeline) = self.pipelines.remove(&id) {
                    self.context
                        .with_gl(|gl| unsafe { gl.delete_program(pipeline.program) });
                }
            }
            ResourceHandle::Material(id) => {
                if let Some(program) = self.materials.remove(&id) {
                    self.context
                        .with_gl(|gl| unsafe { gl.delete_program(program) });
                }
            }
            ResourceHandle::Mesh(id) => {
                if let Some(mesh) = self.meshes.remove(&id) {
                    self.context.with_gl(|gl| unsafe {
                        if let Some(buffer) = mesh.vertex {
                            gl.delete_buffer(buffer);
                        }
                        if let Some(buffer) = mesh.index {
                            gl.delete_buffer(buffer);
                        }
                    });
                }
            }
            ResourceHandle::Sampler(id) => {
                if let Some(sampler) = self.samplers.remove(&id) {
                    self.context
                        .with_gl(|gl| unsafe { gl.delete_sampler(sampler) });
                }
            }
            ResourceHandle::BindGroupLayout(id) => {
                self.layouts.remove(&id);
            }
            ResourceHandle::BindGroup(id) => {
                self.groups.remove(&id);
            }
            ResourceHandle::ComputePipeline(id) => {
                self.compute.remove(&id);
            }
            ResourceHandle::QuerySet(id) => {
                self.queries.remove(&id);
            }
            ResourceHandle::Shader(id) => {
                self.shaders.remove(&id);
            }
            _ => {}
        }
        Ok(())
    }

    fn execute_one(&mut self, command: &RhiCommand) -> Result<(), Error> {
        match command {
            RhiCommand::BeginFrame { .. } | RhiCommand::SubmitFrame { .. } => Ok(()),
            RhiCommand::AcquirePresentationTarget => Ok(()),
            RhiCommand::Barrier { .. } => Ok(()),
            RhiCommand::PushConstants { .. } => {
                Err(Self::absent("push constants are absent on WebGL2"))
            }
            RhiCommand::Dispatch { .. }
            | RhiCommand::BeginComputePass { .. }
            | RhiCommand::EndComputePass
            | RhiCommand::SetComputePipeline { .. } => {
                Err(Self::absent("compute is absent on WebGL2"))
            }
            RhiCommand::WriteTimestamp { .. } | RhiCommand::ResolveQuerySet { .. } => {
                Err(Self::absent("queries are absent on WebGL2"))
            }
            RhiCommand::WriteBuffer {
                buffer,
                offset,
                data,
            } => {
                let gpu = self
                    .buffers
                    .get_mut(buffer)
                    .ok_or_else(|| Error::unsupported("buffer not found"))?;
                let end = *offset as usize + data.len();
                if gpu.bytes.len() < end {
                    gpu.bytes.resize(end, 0);
                }
                gpu.bytes[*offset as usize..end].copy_from_slice(data);
                let handle = gpu.buffer;
                self.context.with_gl(|gl| unsafe {
                    gl.bind_buffer(glow::COPY_WRITE_BUFFER, Some(handle));
                    gl.buffer_sub_data_u8_slice(glow::COPY_WRITE_BUFFER, *offset as i32, data);
                });
                Ok(())
            }
            RhiCommand::CopyBufferToBuffer {
                src,
                src_offset,
                dst,
                dst_offset,
                size,
            } => {
                let src_bytes = self
                    .buffers
                    .get(src)
                    .ok_or_else(|| Error::unsupported("source buffer not found"))?
                    .bytes
                    .clone();
                let src_handle = self.buffers.get(src).map(|buffer| buffer.buffer);
                let dst_buffer = self
                    .buffers
                    .get_mut(dst)
                    .ok_or_else(|| Error::unsupported("destination buffer not found"))?;
                let start = *src_offset as usize;
                let end = start + *size as usize;
                let dst_start = *dst_offset as usize;
                let dst_end = dst_start + *size as usize;
                if src_bytes.len() < end || dst_buffer.bytes.len() < dst_end {
                    return Err(Error::unsupported("buffer copy is out of range"));
                }
                dst_buffer.bytes[dst_start..dst_end].copy_from_slice(&src_bytes[start..end]);
                let dst_handle = dst_buffer.buffer;
                if let Some(src_handle) = src_handle {
                    self.context.with_gl(|gl| unsafe {
                        gl.bind_buffer(glow::COPY_READ_BUFFER, Some(src_handle));
                        gl.bind_buffer(glow::COPY_WRITE_BUFFER, Some(dst_handle));
                        gl.copy_buffer_sub_data(
                            glow::COPY_READ_BUFFER,
                            glow::COPY_WRITE_BUFFER,
                            *src_offset as i32,
                            *dst_offset as i32,
                            *size as i32,
                        );
                    });
                }
                Ok(())
            }
            RhiCommand::CopyBufferToTexture {
                src,
                src_offset,
                dst,
                bytes_per_row,
                width,
                height,
            } => {
                let src_bytes = self
                    .buffers
                    .get(src)
                    .ok_or_else(|| Error::unsupported("source buffer not found"))?
                    .bytes
                    .clone();
                let texture = self
                    .textures
                    .get(dst)
                    .ok_or_else(|| Error::unsupported("destination texture not found"))?;
                let handle = texture.texture;
                let row = if *bytes_per_row == 0 {
                    width * 4
                } else {
                    *bytes_per_row
                };
                let start = *src_offset as usize;
                let tightly = (*height * row) as usize;
                if src_bytes.len() < start + tightly {
                    return Err(Error::unsupported("texture upload is out of range"));
                }
                let pixels = src_bytes[start..start + tightly].to_vec();
                self.context.with_gl(|gl| unsafe {
                    gl.bind_texture(glow::TEXTURE_2D, Some(handle));
                    gl.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        0,
                        0,
                        *width as i32,
                        *height as i32,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&pixels)),
                    );
                });
                Ok(())
            }
            RhiCommand::CopyTextureToBuffer {
                src,
                dst,
                dst_offset,
                width,
                height,
                ..
            } => {
                let readback = self.read_texture(*src)?;
                let dst_buffer = self
                    .buffers
                    .get_mut(dst)
                    .ok_or_else(|| Error::unsupported("destination buffer not found"))?;
                let start = *dst_offset as usize;
                let end = start + readback.data.len().min((*width * *height * 4) as usize);
                if dst_buffer.bytes.len() < end {
                    dst_buffer.bytes.resize(end, 0);
                }
                let count = end - start;
                dst_buffer.bytes[start..end].copy_from_slice(&readback.data[..count]);
                Ok(())
            }
            RhiCommand::BeginRenderPass { pass } => {
                let framebuffer = match pass.color_target {
                    PassColorTarget::Presentation => None,
                    PassColorTarget::Texture(id) => {
                        let texture = self
                            .textures
                            .get(&id)
                            .ok_or_else(|| Error::unsupported("render target texture not found"))?;
                        texture.framebuffer
                    }
                };
                let clear = pass.clear_color;
                let do_clear = matches!(pass.color_load, rotex_types::ColorAttachmentLoad::Clear);
                self.draw_framebuffer = framebuffer;
                self.context.with_gl(|gl| unsafe {
                    gl.bind_framebuffer(glow::FRAMEBUFFER, framebuffer);
                    if do_clear {
                        gl.clear_color(clear[0], clear[1], clear[2], clear[3]);
                        gl.clear(glow::COLOR_BUFFER_BIT);
                    }
                });
                Ok(())
            }
            RhiCommand::EndRenderPass => {
                self.draw_framebuffer = None;
                self.context
                    .with_gl(|gl| unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, None) });
                Ok(())
            }
            RhiCommand::SetViewport {
                x,
                y,
                width,
                height,
                ..
            } => {
                self.context.with_gl(|gl| unsafe {
                    gl.viewport(*x as i32, *y as i32, *width as i32, *height as i32);
                });
                Ok(())
            }
            RhiCommand::SetScissor {
                x,
                y,
                width,
                height,
            } => {
                self.context.with_gl(|gl| unsafe {
                    gl.enable(glow::SCISSOR_TEST);
                    gl.scissor(*x as i32, *y as i32, *width as i32, *height as i32);
                });
                Ok(())
            }
            RhiCommand::SetGraphicsPipeline { pipeline } => {
                let program = self
                    .pipelines
                    .get(pipeline)
                    .ok_or_else(|| Error::unsupported("graphics pipeline not found"))?
                    .program;
                self.bound_pipeline = Some(*pipeline);
                self.context
                    .with_gl(|gl| unsafe { gl.use_program(Some(program)) });
                Ok(())
            }
            RhiCommand::BindVertexBuffer {
                buffer,
                binding,
                offset,
            } => {
                if !self.buffers.contains_key(buffer) {
                    return Err(Error::unsupported("vertex buffer not found"));
                }
                self.vertex_bindings.insert(*binding, (*buffer, *offset));
                Ok(())
            }
            RhiCommand::BindIndexBuffer {
                buffer,
                offset,
                format,
            } => {
                if !self.buffers.contains_key(buffer) {
                    return Err(Error::unsupported("index buffer not found"));
                }
                self.index_binding = Some((*buffer, *offset, *format));
                Ok(())
            }
            RhiCommand::BindGroups {
                bind_groups,
                dynamic_offsets,
                ..
            } => {
                let mut offset_index = 0;
                let groups: Vec<_> = bind_groups.clone();
                for group_id in groups {
                    let Some(group) = self.groups.get(&group_id) else {
                        return Err(Error::unsupported("bind group not found"));
                    };
                    let binds = group.buffers.clone();
                    for (binding, buffer, base, size) in binds {
                        let dynamic = dynamic_offsets.get(offset_index).copied().unwrap_or(0);
                        offset_index += 1;
                        let Some(gpu) = self.buffers.get(&buffer) else {
                            return Err(Error::unsupported("bound buffer not found"));
                        };
                        let handle = gpu.buffer;
                        let range = if size == 0 { 256 } else { size as i32 };
                        self.context.with_gl(|gl| unsafe {
                            gl.bind_buffer_range(
                                glow::UNIFORM_BUFFER,
                                binding,
                                Some(handle),
                                (base as u32).saturating_add(dynamic) as i32,
                                range,
                            );
                        });
                    }
                }
                Ok(())
            }
            RhiCommand::Draw {
                vertex_count,
                instance_count,
                first_vertex,
                first_instance,
            } => {
                self.prepare_draw()?;
                let topology = self.bound_topology();
                self.context.with_gl(|gl| unsafe {
                    if *instance_count > 1 || *first_instance > 0 {
                        gl.draw_arrays_instanced_base_instance(
                            topology,
                            *first_vertex as i32,
                            *vertex_count as i32,
                            *instance_count as i32,
                            *first_instance,
                        );
                    } else {
                        gl.draw_arrays(topology, *first_vertex as i32, *vertex_count as i32);
                    }
                });
                Ok(())
            }
            RhiCommand::DrawIndexed {
                index_count,
                instance_count,
                first_index,
                vertex_offset,
                first_instance,
            } => {
                self.prepare_draw()?;
                let (index_type, index_offset) = self.bind_index()?;
                let topology = self.bound_topology();
                let index_size = if index_type == glow::UNSIGNED_SHORT {
                    2
                } else {
                    4
                };
                let offset = index_offset + (*first_index as i32) * index_size;
                self.context.with_gl(|gl| unsafe {
                    gl.draw_elements_instanced_base_vertex_base_instance(
                        topology,
                        *index_count as i32,
                        index_type,
                        offset,
                        *instance_count as i32,
                        *vertex_offset,
                        *first_instance,
                    );
                });
                Ok(())
            }
            RhiCommand::SetVertexBuffers { mesh, .. } => {
                let mesh = self
                    .meshes
                    .get(mesh)
                    .ok_or_else(|| Error::unsupported("mesh not found"))?;
                if let Some(buffer) = mesh.vertex {
                    self.context.with_gl(|gl| unsafe {
                        gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                    });
                }
                Ok(())
            }
            RhiCommand::SetIndexBuffer { mesh } => {
                let mesh = self
                    .meshes
                    .get(mesh)
                    .ok_or_else(|| Error::unsupported("mesh not found"))?;
                if let Some(buffer) = mesh.index {
                    self.context.with_gl(|gl| unsafe {
                        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(buffer));
                    });
                }
                Ok(())
            }
            RhiCommand::Extension { .. } => {
                Err(Error::unsupported("extension command is not implemented"))
            }
            _ => Err(Error::unsupported("command is not implemented")),
        }
    }

    fn bound_topology(&self) -> u32 {
        self.bound_pipeline
            .and_then(|id| self.pipelines.get(&id))
            .map(|pipeline| pipeline.topology)
            .unwrap_or(glow::TRIANGLES)
    }

    fn prepare_draw(&mut self) -> Result<(), Error> {
        let Some(pipeline_id) = self.bound_pipeline else {
            return Ok(());
        };
        let layouts = self
            .pipelines
            .get(&pipeline_id)
            .map(|pipeline| pipeline.layouts.clone())
            .unwrap_or_default();
        let mut attribs = Vec::new();
        for (binding, layout) in layouts.iter().enumerate() {
            let Some((buffer, offset)) = self.vertex_bindings.get(&(binding as u32)) else {
                continue;
            };
            let Some(gpu) = self.buffers.get(buffer) else {
                continue;
            };
            let divisor = match layout.step_mode {
                VertexStepMode::Vertex => 0,
                VertexStepMode::Instance => 1,
            };
            for attribute in &layout.attributes {
                let (size, ty, normalized) = match attribute.format {
                    VertexFormat::Float32 => (1, glow::FLOAT, false),
                    VertexFormat::Float32x2 => (2, glow::FLOAT, false),
                    VertexFormat::Float32x3 => (3, glow::FLOAT, false),
                    VertexFormat::Float32x4 => (4, glow::FLOAT, false),
                    VertexFormat::Uint32 | VertexFormat::Sint32 => (1, glow::UNSIGNED_INT, false),
                    VertexFormat::Unorm8x4 => (4, glow::UNSIGNED_BYTE, true),
                    VertexFormat::Float16x2 => (2, glow::HALF_FLOAT, false),
                };
                attribs.push((
                    gpu.buffer,
                    attribute.location,
                    size,
                    ty,
                    normalized,
                    layout.array_stride as i32,
                    (*offset + attribute.offset) as i32,
                    divisor,
                ));
            }
        }
        self.context.with_gl(|gl| unsafe {
            for (buffer, location, size, ty, normalized, stride, offset, divisor) in attribs {
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                gl.enable_vertex_attrib_array(location);
                gl.vertex_attrib_pointer_f32(location, size, ty, normalized, stride, offset);
                gl.vertex_attrib_divisor(location, divisor);
            }
        });
        Ok(())
    }

    fn bind_index(&self) -> Result<(u32, i32), Error> {
        let Some((buffer, offset, format)) = self.index_binding else {
            return Err(Error::unsupported("index buffer is not bound"));
        };
        let handle = self
            .buffers
            .get(&buffer)
            .ok_or_else(|| Error::unsupported("index buffer not found"))?
            .buffer;
        self.context.with_gl(|gl| unsafe {
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(handle));
        });
        let ty = match format {
            IndexFormat::Uint16 => glow::UNSIGNED_SHORT,
            IndexFormat::Uint32 => glow::UNSIGNED_INT,
        };
        Ok((ty, offset as i32))
    }

    fn link_program(
        &self,
        vertex: &rotex_types::ShaderPackage,
        fragment: &rotex_types::ShaderPackage,
    ) -> Result<glow::Program, Error> {
        let vertex_source = glsl_source(vertex)?;
        let fragment_source = glsl_source(fragment)?;
        self.context.with_gl(|gl| unsafe {
            let program = gl
                .create_program()
                .map_err(|_| Error::unsupported("program allocation failed"))?;
            let vertex_shader = compile(gl, glow::VERTEX_SHADER, vertex_source)?;
            let fragment_shader = compile(gl, glow::FRAGMENT_SHADER, fragment_source)?;
            gl.attach_shader(program, vertex_shader);
            gl.attach_shader(program, fragment_shader);
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                gl.delete_shader(vertex_shader);
                gl.delete_shader(fragment_shader);
                gl.delete_program(program);
                let _ = log;
                return Err(Error::unsupported("glsl program link failed"));
            }
            gl.delete_shader(vertex_shader);
            gl.delete_shader(fragment_shader);
            Ok(program)
        })
    }
}

fn compile(gl: &glow::Context, stage: u32, source: &str) -> Result<glow::Shader, Error> {
    unsafe {
        let shader = gl
            .create_shader(stage)
            .map_err(|_| Error::unsupported("shader allocation failed"))?;
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            gl.delete_shader(shader);
            return Err(Error::unsupported("glsl compile failed"));
        }
        Ok(shader)
    }
}

fn glsl_source(package: &rotex_types::ShaderPackage) -> Result<&str, Error> {
    match package.variants.glsl.as_ref() {
        Some(ShaderPayload::Glsl(source)) => Ok(source.as_str()),
        _ => Err(Error::unsupported("glsl shader payload is missing")),
    }
}

fn ensure_absent<K, V>(map: &HashMap<K, V>, key: &K) -> Result<(), Error>
where
    K: Eq + std::hash::Hash,
{
    if map.contains_key(key) {
        Err(Error::unsupported("resource id already exists"))
    } else {
        Ok(())
    }
}
