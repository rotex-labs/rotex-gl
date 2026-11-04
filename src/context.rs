//! EGL (native) or WebGL2 (wasm) context kept current for glow calls.

use std::ffi::{CString, c_void};

use rotex_core::Error;

pub(crate) struct GlContext {
    #[cfg(not(target_arch = "wasm32"))]
    egl: Egl,
    gl: glow::Context,
}

impl GlContext {
    pub(crate) fn new() -> Result<Self, Error> {
        #[cfg(target_arch = "wasm32")]
        {
            web_context()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            native_context()
        }
    }

    pub(crate) fn with_gl<R>(&self, body: impl FnOnce(&glow::Context) -> R) -> R {
        #[cfg(not(target_arch = "wasm32"))]
        self.egl.make_current();
        body(&self.gl)
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Egl {
    _library: libloading::Library,
    make_current: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> u32,
    destroy_surface: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32,
    destroy_context: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32,
    terminate: unsafe extern "C" fn(*mut c_void) -> u32,
    display: *mut c_void,
    surface: *mut c_void,
    context: *mut c_void,
}

#[cfg(not(target_arch = "wasm32"))]
unsafe impl Send for Egl {}
unsafe impl Sync for Egl {}

#[cfg(not(target_arch = "wasm32"))]
impl Egl {
    fn make_current(&self) {
        unsafe {
            (self.make_current)(self.display, self.surface, self.surface, self.context);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Egl {
    fn drop(&mut self) {
        unsafe {
            (self.make_current)(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            (self.destroy_surface)(self.display, self.surface);
            (self.destroy_context)(self.display, self.context);
            (self.terminate)(self.display);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_context() -> Result<GlContext, Error> {
    let library = unsafe { libloading::Library::new("libEGL.so.1") }
        .or_else(|_| unsafe { libloading::Library::new("libEGL.so") })
        .map_err(|_| Error::unsupported("libEGL is not available"))?;
    unsafe {
        type DisplayFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
        type InitFn = unsafe extern "C" fn(*mut c_void, *mut i32, *mut i32) -> u32;
        type BindFn = unsafe extern "C" fn(u32) -> u32;
        type ChooseFn =
            unsafe extern "C" fn(*mut c_void, *const i32, *mut *mut c_void, i32, *mut i32) -> u32;
        type CreateCtxFn =
            unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const i32) -> *mut c_void;
        type CreateSurfFn =
            unsafe extern "C" fn(*mut c_void, *mut c_void, *const i32) -> *mut c_void;
        type MakeFn =
            unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> u32;
        type ProcFn = unsafe extern "C" fn(*const i8) -> *mut c_void;
        type DestroySurfFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32;
        type DestroyCtxFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32;
        type TerminateFn = unsafe extern "C" fn(*mut c_void) -> u32;

        let get_display: libloading::Symbol<DisplayFn> = library
            .get(b"eglGetDisplay\0")
            .map_err(|_| Error::unsupported("eglGetDisplay is missing"))?;
        let initialize: libloading::Symbol<InitFn> = library
            .get(b"eglInitialize\0")
            .map_err(|_| Error::unsupported("eglInitialize is missing"))?;
        let bind_api: libloading::Symbol<BindFn> = library
            .get(b"eglBindAPI\0")
            .map_err(|_| Error::unsupported("eglBindAPI is missing"))?;
        let choose_config: libloading::Symbol<ChooseFn> = library
            .get(b"eglChooseConfig\0")
            .map_err(|_| Error::unsupported("eglChooseConfig is missing"))?;
        let create_context: libloading::Symbol<CreateCtxFn> = library
            .get(b"eglCreateContext\0")
            .map_err(|_| Error::unsupported("eglCreateContext is missing"))?;
        let create_pbuffer: libloading::Symbol<CreateSurfFn> = library
            .get(b"eglCreatePbufferSurface\0")
            .map_err(|_| Error::unsupported("eglCreatePbufferSurface is missing"))?;
        let make_current: libloading::Symbol<MakeFn> = library
            .get(b"eglMakeCurrent\0")
            .map_err(|_| Error::unsupported("eglMakeCurrent is missing"))?;
        let get_proc: libloading::Symbol<ProcFn> = library
            .get(b"eglGetProcAddress\0")
            .map_err(|_| Error::unsupported("eglGetProcAddress is missing"))?;
        let destroy_surface: libloading::Symbol<DestroySurfFn> = library
            .get(b"eglDestroySurface\0")
            .map_err(|_| Error::unsupported("eglDestroySurface is missing"))?;
        let destroy_context: libloading::Symbol<DestroyCtxFn> = library
            .get(b"eglDestroyContext\0")
            .map_err(|_| Error::unsupported("eglDestroyContext is missing"))?;
        let terminate: libloading::Symbol<TerminateFn> = library
            .get(b"eglTerminate\0")
            .map_err(|_| Error::unsupported("eglTerminate is missing"))?;

        const EGL_NONE: i32 = 0x3038;
        const EGL_OPENGL_ES_API: u32 = 0x30A0;
        const EGL_SURFACE_TYPE: i32 = 0x3033;
        const EGL_PBUFFER_BIT: i32 = 0x0001;
        const EGL_RENDERABLE_TYPE: i32 = 0x3040;
        const EGL_OPENGL_ES3_BIT: i32 = 0x40;
        const EGL_RED_SIZE: i32 = 0x3024;
        const EGL_GREEN_SIZE: i32 = 0x3023;
        const EGL_BLUE_SIZE: i32 = 0x3022;
        const EGL_ALPHA_SIZE: i32 = 0x3021;
        const EGL_CONTEXT_MAJOR_VERSION: i32 = 0x3098;
        const EGL_CONTEXT_MINOR_VERSION: i32 = 0x30FB;
        const EGL_WIDTH: i32 = 0x3057;
        const EGL_HEIGHT: i32 = 0x3056;

        let display = get_display(std::ptr::null_mut());
        if display.is_null() {
            return Err(Error::unsupported("eglGetDisplay failed"));
        }
        let mut major = 0;
        let mut minor = 0;
        if initialize(display, &mut major, &mut minor) == 0 {
            return Err(Error::unsupported("eglInitialize failed"));
        }
        if bind_api(EGL_OPENGL_ES_API) == 0 {
            return Err(Error::unsupported("eglBindAPI failed"));
        }
        let config_attribs = [
            EGL_SURFACE_TYPE,
            EGL_PBUFFER_BIT,
            EGL_RENDERABLE_TYPE,
            EGL_OPENGL_ES3_BIT,
            EGL_RED_SIZE,
            8,
            EGL_GREEN_SIZE,
            8,
            EGL_BLUE_SIZE,
            8,
            EGL_ALPHA_SIZE,
            8,
            EGL_NONE,
        ];
        let mut config = std::ptr::null_mut();
        let mut count = 0;
        if choose_config(display, config_attribs.as_ptr(), &mut config, 1, &mut count) == 0
            || count == 0
            || config.is_null()
        {
            return Err(Error::unsupported("eglChooseConfig failed"));
        }
        let context_attribs = [
            EGL_CONTEXT_MAJOR_VERSION,
            3,
            EGL_CONTEXT_MINOR_VERSION,
            0,
            EGL_NONE,
        ];
        let context = create_context(
            display,
            config,
            std::ptr::null_mut(),
            context_attribs.as_ptr(),
        );
        if context.is_null() {
            return Err(Error::unsupported("eglCreateContext failed"));
        }
        let surface_attribs = [EGL_WIDTH, 64, EGL_HEIGHT, 64, EGL_NONE];
        let surface = create_pbuffer(display, config, surface_attribs.as_ptr());
        if surface.is_null() {
            return Err(Error::unsupported("eglCreatePbufferSurface failed"));
        }
        let make_current_fn = *make_current;
        let get_proc_fn = *get_proc;
        let destroy_surface_fn = *destroy_surface;
        let destroy_context_fn = *destroy_context;
        let terminate_fn = *terminate;
        // `Symbol` does not implement `Drop`. Ending these bindings releases the
        // borrow of `library` so the library can move into the context.
        #[allow(clippy::drop_non_drop)]
        {
            drop(get_display);
            drop(initialize);
            drop(bind_api);
            drop(choose_config);
            drop(create_context);
            drop(create_pbuffer);
            drop(make_current);
            drop(get_proc);
            drop(destroy_surface);
            drop(destroy_context);
            drop(terminate);
        }
        if make_current_fn(display, surface, surface, context) == 0 {
            return Err(Error::unsupported("eglMakeCurrent failed"));
        }
        let gl = glow::Context::from_loader_function(|name| {
            CString::new(name).map_or(std::ptr::null_mut(), |symbol| get_proc_fn(symbol.as_ptr()))
        });
        Ok(GlContext {
            egl: Egl {
                _library: library,
                make_current: make_current_fn,
                destroy_surface: destroy_surface_fn,
                destroy_context: destroy_context_fn,
                terminate: terminate_fn,
                display,
                surface,
                context,
            },
            gl,
        })
    }
}

#[cfg(target_arch = "wasm32")]
fn web_context() -> Result<GlContext, Error> {
    use wasm_bindgen::JsCast;
    let window = web_sys::window().ok_or_else(|| Error::unsupported("window is missing"))?;
    let document = window
        .document()
        .ok_or_else(|| Error::unsupported("document is missing"))?;
    let canvas = document
        .create_element("canvas")
        .map_err(|_| Error::unsupported("canvas creation failed"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| Error::unsupported("canvas cast failed"))?;
    canvas.set_width(64);
    canvas.set_height(64);
    let gl_context = canvas
        .get_context("webgl2")
        .map_err(|_| Error::unsupported("webgl2 context failed"))?
        .ok_or_else(|| Error::unsupported("webgl2 context is missing"))?;
    let gl_context = gl_context
        .dyn_into::<web_sys::WebGl2RenderingContext>()
        .map_err(|_| Error::unsupported("webgl2 cast failed"))?;
    Ok(GlContext {
        gl: glow::Context::from_webgl2_context(gl_context),
    })
}
