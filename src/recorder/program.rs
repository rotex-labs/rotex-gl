use super::*;

impl GlBridge {
    pub(super) fn link_program(
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

pub(super) fn ensure_absent<K, V>(map: &HashMap<K, V>, key: &K) -> Result<(), Error>
where
    K: Eq + std::hash::Hash,
{
    if map.contains_key(key) {
        Err(Error::unsupported("resource id already exists"))
    } else {
        Ok(())
    }
}
