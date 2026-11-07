use super::program::ensure_absent;
use super::*;

impl GlBridge {
    pub(super) fn create_one(
        &mut self,
        resource: ResourceCreateDescriptor,
    ) -> Result<ResourceHandle, Error> {
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

    pub(super) fn update_one(&mut self, update: ResourceUpdateDescriptor) -> Result<(), Error> {
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

    pub(super) fn destroy_handle(&mut self, handle: ResourceHandle) -> Result<(), Error> {
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
}
