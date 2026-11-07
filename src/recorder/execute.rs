use super::*;

impl GlBridge {
    pub(super) fn execute_one(&mut self, command: &RhiCommand) -> Result<(), Error> {
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

    pub(super) fn bound_topology(&self) -> u32 {
        self.bound_pipeline
            .and_then(|id| self.pipelines.get(&id))
            .map(|pipeline| pipeline.topology)
            .unwrap_or(glow::TRIANGLES)
    }

    pub(super) fn prepare_draw(&mut self) -> Result<(), Error> {
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

    pub(super) fn bind_index(&self) -> Result<(u32, i32), Error> {
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

}
