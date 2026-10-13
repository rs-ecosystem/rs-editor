//! The JNI half of the shell, for what needs Java objects: the drawing
//! surface (EGL on an `android.view.Surface`) and AccessKit (which installs
//! itself as the view's accessibility delegate). Everything else goes
//! through the BoltFFI class in `api.rs`.
//!
//! `handle` is the BoltFFI handle of a [`NativeEditor`] (`NativeEditor.handle`
//! in Kotlin); calls happen on the UI thread.

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeId, TreeUpdate};
use accesskit_android::InjectingAdapter;
use jni::objects::{GlobalRef, JClass, JObject};
use jni::sys::{jboolean, jfloat, jint, jlong};
use jni::{JNIEnv, JavaVM};
use khronos_egl as egl;
use ndk::native_window::NativeWindow;
use rs_editor_view::EditorView;
use skia_safe::gpu::{self, DirectContext, SurfaceOrigin, gl::FramebufferInfo};
use skia_safe::{ColorType, Surface};

use crate::api::NativeEditor;

struct Gl {
    egl: egl::Instance<egl::Static>,
    display: egl::Display,
    config: egl::Config,
    context: egl::Context,
    window: Option<(egl::Surface, NativeWindow)>,
    skia: Option<DirectContext>,
    target: Option<Surface>,
    pixels: (i32, i32),
}

/// What the Android side keeps per editor.
#[derive(Default)]
pub(crate) struct Shell {
    gl: Option<Gl>,
    /// TalkBack and other accessibility services (AccessKit).
    accessibility: Option<InjectingAdapter>,
    /// A `Runnable` told about changes made by accessibility actions.
    changed: Option<(JavaVM, GlobalRef)>,
}

/// Answers AccessKit on the UI thread, where the Java delegate calls in.
struct Accessibility(jlong);

impl ActivationHandler for Accessibility {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(app(self.0).view.accessibility_tree())
    }
}

impl ActionHandler for Accessibility {
    fn do_action(&mut self, request: ActionRequest) {
        let app = app(self.0);
        let handled = app.view.accessibility_action(&request);
        if !handled {
            return;
        }
        // Let the view redraw and resync the IME.
        if let Some((vm, changed)) = &app.shell.changed
            && let Ok(mut env) = vm.attach_current_thread()
        {
            let _ = env.call_method(changed.as_obj(), "run", "()V", &[]);
        }
    }
}

fn app<'a>(handle: jlong) -> &'a mut NativeEditor {
    // SAFETY: BoltFFI handles are `Box<NativeEditor>` pointers that live
    // until `close()`; Kotlin only calls on the UI thread, with a live handle.
    unsafe { &mut *(handle as *mut NativeEditor) }
}

impl Drop for Gl {
    fn drop(&mut self) {
        self.detach();
        self.skia = None;
        let _ = self.egl.destroy_context(self.display, self.context);
    }
}

impl Gl {
    fn new() -> Result<Self, egl::Error> {
        let egl = egl::Instance::new(egl::Static);
        // SAFETY: the default display is always valid to request.
        let display =
            unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }.ok_or(egl::Error::BadDisplay)?;
        egl.initialize(display)?;
        let attributes = [
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            8,
            egl::STENCIL_SIZE,
            8,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_ES3_BIT,
            egl::SURFACE_TYPE,
            egl::WINDOW_BIT,
            egl::NONE,
        ];
        let config = egl
            .choose_first_config(display, &attributes)?
            .ok_or(egl::Error::BadConfig)?;
        let context_attributes = [egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE];
        let context = egl.create_context(display, config, None, &context_attributes)?;
        Ok(Self {
            egl,
            display,
            config,
            context,
            window: None,
            skia: None,
            target: None,
            pixels: (1, 1),
        })
    }

    fn attach(&mut self, window: NativeWindow) -> Result<(), egl::Error> {
        self.detach();
        // SAFETY: the native window stays alive while we hold it.
        let surface = unsafe {
            self.egl.create_window_surface(
                self.display,
                self.config,
                window.ptr().as_ptr() as egl::NativeWindowType,
                None,
            )
        }?;
        self.egl.make_current(
            self.display,
            Some(surface),
            Some(surface),
            Some(self.context),
        )?;
        self.pixels = (window.width(), window.height());
        self.window = Some((surface, window));
        if self.skia.is_none() {
            self.skia = gpu::gl::Interface::new_native()
                .and_then(|interface| gpu::direct_contexts::make_gl(interface, None));
        }
        self.target = None;
        Ok(())
    }

    fn detach(&mut self) {
        self.target = None;
        if let Some((surface, _window)) = self.window.take() {
            let _ = self
                .egl
                .make_current(self.display, None, None, Some(self.context));
            let _ = self.egl.destroy_surface(self.display, surface);
        }
    }

    /// The size of the buffers EGL currently draws into, which only follows
    /// a window resize after the next swap.
    fn surface_size(&self, surface: egl::Surface) -> Option<(i32, i32)> {
        let width = self
            .egl
            .query_surface(self.display, surface, egl::WIDTH)
            .ok()?;
        let height = self
            .egl
            .query_surface(self.display, surface, egl::HEIGHT)
            .ok()?;
        Some((width, height))
    }

    /// Draws a frame; returns whether another one is needed because the
    /// surface is still changing size.
    fn frame(&mut self, view: &mut EditorView) -> bool {
        let Some((surface, _)) = &self.window else {
            return false;
        };
        let surface = *surface;
        if self
            .egl
            .make_current(
                self.display,
                Some(surface),
                Some(surface),
                Some(self.context),
            )
            .is_err()
        {
            return false;
        }
        if let Some(size) = self.surface_size(surface)
            && size != self.pixels
        {
            self.pixels = size;
            self.target = None;
        }
        let Some(skia) = self.skia.as_mut() else {
            return false;
        };
        if self.target.is_none() {
            let target = gpu::backend_render_targets::make_gl(
                self.pixels,
                0,
                8,
                FramebufferInfo {
                    fboid: 0,
                    format: gpu::gl::Format::RGBA8.into(),
                    protected: gpu::Protected::No,
                },
            );
            self.target = gpu::surfaces::wrap_backend_render_target(
                skia,
                &target,
                SurfaceOrigin::BottomLeft,
                ColorType::RGBA8888,
                None,
                None,
            );
        }
        let Some(target) = self.target.as_mut() else {
            return false;
        };
        view.paint(target.canvas());
        skia.flush_and_submit();
        let _ = self.egl.swap_buffers(self.display, surface);
        self.surface_size(surface)
            .is_some_and(|size| size != self.pixels)
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rseditor_android_NativeSurface_attachSurface(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    surface: JObject,
) -> jboolean {
    let app = app(handle);
    // SAFETY: `surface` is a live android.view.Surface for this call.
    let Some(window) = (unsafe { NativeWindow::from_surface(env.get_raw(), surface.as_raw()) })
    else {
        return 0;
    };
    if app.shell.gl.is_none() {
        match Gl::new() {
            Ok(gl) => app.shell.gl = Some(gl),
            Err(_) => return 0,
        }
    }
    let Some(gl) = app.shell.gl.as_mut() else {
        return 0;
    };
    jboolean::from(gl.attach(window).is_ok() && gl.skia.is_some())
}

/// The surface changed size (`surfaceChanged`), in pixels.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rseditor_android_NativeSurface_resize(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    width: jint,
    height: jint,
    density: jfloat,
) {
    let app = app(handle);
    app.view
        .resize(width as f32 / density, height as f32 / density, density);
    if let Some(gl) = app.shell.gl.as_mut() {
        gl.pixels = (width, height);
        gl.target = None;
    }
}

/// The surface is going away (`surfaceDestroyed`); the GL context survives.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rseditor_android_NativeSurface_detachSurface(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(gl) = app(handle).shell.gl.as_mut() {
        gl.detach();
    }
}

/// Draws a frame. Returns milliseconds until the next one is needed
/// (0 while animating) or -1 when idle.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rseditor_android_NativeSurface_render(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    let app = app(handle);
    let resizing = app
        .shell
        .gl
        .as_mut()
        .is_some_and(|gl| gl.frame(&mut app.view));
    // Changes since the last frame, while an accessibility service listens.
    if let Some(adapter) = app.shell.accessibility.as_mut() {
        let view = &mut app.view;
        adapter.update_if_active(|| {
            view.accessibility_update().unwrap_or(TreeUpdate {
                nodes: Vec::new(),
                tree: None,
                tree_id: TreeId::ROOT,
                focus: rs_editor_view::EDITOR_NODE,
            })
        });
    }
    if resizing {
        return 0;
    }
    app.view.next_frame_in().map_or(-1, |ms| ms as jlong)
}

/// Makes the editor visible to TalkBack: AccessKit installs itself as the
/// view's accessibility delegate. `changed` (a `Runnable`) runs after an
/// accessibility action edits the document.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rseditor_android_NativeSurface_attachAccessibility(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    view: JObject,
    changed: JObject,
) {
    let app = app(handle);
    if app.shell.accessibility.is_some() {
        return;
    }
    if let (Ok(vm), Ok(changed)) = (env.get_java_vm(), env.new_global_ref(&changed)) {
        app.shell.changed = Some((vm, changed));
    }
    app.shell.accessibility = Some(InjectingAdapter::new(
        &mut env,
        &view,
        Accessibility(handle),
        Accessibility(handle),
    ));
}
