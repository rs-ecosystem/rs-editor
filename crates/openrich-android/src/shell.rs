//! JNI entry points for `dev.openrich.android.OpenRichNative`.
//!
//! Every method takes the handle returned by `create` and must be called on
//! the UI thread (Skia objects are not thread-safe).

use std::time::{SystemTime, UNIX_EPOCH};

use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jfloat, jint, jlong, jstring};
use khronos_egl as egl;
use ndk::native_window::NativeWindow;
use openrich_commands::{KeyEvent, KeyPlatform, Modifiers};
use openrich_extensions::EditorOptions;
use openrich_theme::Theme;
use openrich_view::{EditorView, PointerButton, PointerEvent, VelocityTracker, ViewOptions};
use serde_json::{Value, json};
use skia_safe::gpu::{self, DirectContext, SurfaceOrigin, gl::FramebufferInfo};
use skia_safe::{ColorType, Surface};

/// Movement (dp) before a touch becomes a scroll.
const TOUCH_SLOP: f32 = 8.0;
/// A press longer than this selects a word.
const LONG_PRESS_MS: u64 = 450;
/// Taps closer than this in time (and 24 dp in space) count together.
const MULTI_TAP_MS: u64 = 300;

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

struct Touch {
    start: (f32, f32),
    last_y: f32,
    down_ms: u64,
    scrolling: bool,
}

pub struct App {
    view: EditorView,
    gl: Option<Gl>,
    touch: Option<Touch>,
    tracker: VelocityTracker,
    /// Time, place and count of the last tap.
    last_tap: Option<(u64, f32, f32, u32)>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn app<'a>(handle: jlong) -> &'a mut App {
    // SAFETY: handles come from `create` and live until `destroy`; Java only
    // calls on the UI thread.
    unsafe { &mut *(handle as *mut App) }
}

fn string(env: &mut JNIEnv, value: &JString) -> String {
    env.get_string(value).map(Into::into).unwrap_or_default()
}

fn new_string(env: &mut JNIEnv, value: &str) -> jstring {
    env.new_string(value)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
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

impl App {
    fn ime_state(&mut self) -> Value {
        let caret = self.view.caret_rect().map(|r| {
            let scale = self.view.size().2;
            [r.x * scale, r.y * scale, r.width * scale, r.height * scale]
        });
        match self.view.ime_context() {
            Some(context) => json!({
                "start": context.start,
                "text": context.text,
                "selection": [context.selection.0, context.selection.1],
                "composition": context.composition.map(|(a, b)| [a, b]),
                "caret": caret,
            }),
            None => {
                json!({"start": -1, "text": "", "selection": [0, 0], "composition": null, "caret": caret})
            }
        }
    }

    fn tap(&mut self, x: f32, y: f32, clicks: u32) {
        let event = PointerEvent {
            x,
            y,
            button: PointerButton::Primary,
            modifiers: Modifiers::default(),
            click_count: clicks,
        };
        self.view.pointer_down(event);
        self.view.pointer_up(event);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_create(
    mut env: JNIEnv,
    _class: JClass,
    width: jfloat,
    height: jfloat,
    density: jfloat,
    dark: jboolean,
    content: JString,
) -> jlong {
    let content: Option<Value> = serde_json::from_str(&string(&mut env, &content)).ok();
    let Ok(mut view) = EditorView::new(ViewOptions {
        editor: EditorOptions {
            content,
            platform: KeyPlatform::Other,
            ..Default::default()
        },
        theme: if dark != 0 {
            Theme::dark()
        } else {
            Theme::light()
        },
        width: width / density,
        height: height / density,
        scale: density,
        ..Default::default()
    }) else {
        return 0;
    };
    view.set_padding(openrich_view::Insets {
        top: 24.0,
        right: 20.0,
        bottom: 24.0,
        left: 20.0,
    });
    Box::into_raw(Box::new(App {
        view,
        gl: None,
        touch: None,
        tracker: VelocityTracker::new(),
        last_tap: None,
    })) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_destroy(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle != 0 {
        // SAFETY: the handle came from `create` and is not used afterwards.
        let mut app = unsafe { Box::from_raw(handle as *mut App) };
        if let Some(gl) = app.gl.as_mut() {
            gl.detach();
            gl.skia = None;
            let _ = gl.egl.destroy_context(gl.display, gl.context);
        }
    }
}

/// Starts drawing into `surface` (`surfaceCreated`). Returns false when EGL
/// could not be set up.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_attachSurface(
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
    if app.gl.is_none() {
        match Gl::new() {
            Ok(gl) => app.gl = Some(gl),
            Err(_) => return 0,
        }
    }
    let Some(gl) = app.gl.as_mut() else {
        return 0;
    };
    jboolean::from(gl.attach(window).is_ok() && gl.skia.is_some())
}

/// The surface changed size (`surfaceChanged`), in pixels.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_resize(
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
    if let Some(gl) = app.gl.as_mut() {
        gl.pixels = (width, height);
        gl.target = None;
    }
}

/// The surface is going away (`surfaceDestroyed`); the GL context survives.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_detachSurface(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(gl) = app(handle).gl.as_mut() {
        gl.detach();
    }
}

/// Draws a frame. Returns milliseconds until the next one is needed
/// (0 while animating) or -1 when idle.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_render(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    let app = app(handle);
    let resizing = app.gl.as_mut().is_some_and(|gl| gl.frame(&mut app.view));
    if resizing {
        return 0;
    }
    app.view.next_frame_in().map_or(-1, |ms| ms as jlong)
}

/// A touch event in pixels; `action` is 0 down, 1 move, 2 up, 3 cancel.
/// Returns true when the soft keyboard should be shown (a tap on text).
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_touch(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    action: jint,
    x: jfloat,
    y: jfloat,
) -> jboolean {
    let app = app(handle);
    let scale = app.view.size().2;
    let (x, y) = (x / scale, y / scale);
    let now = now_ms();
    match action {
        0 => {
            app.view.stop_scrolling();
            app.tracker.reset();
            app.tracker.add(now, y);
            app.touch = Some(Touch {
                start: (x, y),
                last_y: y,
                down_ms: now,
                scrolling: false,
            });
            0
        }
        1 => {
            let Some(touch) = app.touch.as_mut() else {
                return 0;
            };
            if !touch.scrolling && (x - touch.start.0).hypot(y - touch.start.1) > TOUCH_SLOP {
                touch.scrolling = true;
            }
            if touch.scrolling {
                let dy = touch.last_y - y;
                touch.last_y = y;
                app.tracker.add(now, y);
                app.view.scroll_by(dy);
            }
            0
        }
        2 => {
            let Some(touch) = app.touch.take() else {
                return 0;
            };
            if touch.scrolling {
                app.tracker.add(now, y);
                app.view.fling(-app.tracker.velocity());
                return 0;
            }
            if now - touch.down_ms >= LONG_PRESS_MS {
                app.tap(x, y, 2);
                app.last_tap = None;
                return 1;
            }
            let clicks = match app.last_tap {
                Some((at, tx, ty, count))
                    if now - at <= MULTI_TAP_MS && (x - tx).hypot(y - ty) <= 24.0 =>
                {
                    count % 3 + 1
                }
                _ => 1,
            };
            app.last_tap = Some((now, x, y, clicks));
            app.tap(x, y, clicks);
            1
        }
        _ => {
            app.touch = None;
            0
        }
    }
}

/// A hardware or IME key (`sendKeyEvent`), as a W3C key name.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_keyDown(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    key: JString,
    shift: jboolean,
    ctrl: jboolean,
    alt: jboolean,
) -> jboolean {
    let key = string(&mut env, &key);
    let event = KeyEvent::new(
        &key,
        Modifiers {
            shift: shift != 0,
            ctrl: ctrl != 0,
            alt: alt != 0,
            meta: false,
        },
    );
    jboolean::from(app(handle).view.key_down(&event))
}

/// `commitText`: replaces the composition (or selection) with `text`;
/// newlines split blocks like Enter.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_commitText(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    text: JString,
) {
    let text = string(&mut env, &text);
    let view = &mut app(handle).view;
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            view.key_down(&KeyEvent::new("Enter", Modifiers::default()));
        }
        if view.composition_range().is_some() {
            view.commit_composition(line);
        } else if !line.is_empty() {
            view.insert_text(line);
        }
    }
}

/// `setComposingText`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_setComposingText(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    text: JString,
) {
    let text = string(&mut env, &text);
    app(handle).view.set_composition(&text);
}

/// `setComposingRegion`, in UTF-16 offsets within the IME block.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_setComposingRegion(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    start: jint,
    end: jint,
) {
    app(handle)
        .view
        .ime_set_composing_region(start.max(0) as usize, end.max(0) as usize);
}

/// `finishComposingText`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_finishComposing(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    app(handle).view.ime_finish_composition();
}

/// `deleteSurroundingText`, in UTF-16 units.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_deleteSurrounding(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    before: jint,
    after: jint,
) {
    app(handle)
        .view
        .ime_delete_surrounding(before.max(0) as usize, after.max(0) as usize);
}

/// `setSelection`, in UTF-16 offsets within the IME block.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_setSelection(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    start: jint,
    end: jint,
) {
    app(handle)
        .view
        .ime_set_selection(start.max(0) as usize, end.max(0) as usize);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_setFocused(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    focused: jboolean,
) {
    app(handle).view.set_focused(focused != 0);
}

/// The IME block as JSON: `{start, text, selection, composition, caret}`,
/// with the caret in pixels.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_imeState(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let state = app(handle).ime_state().to_string();
    new_string(&mut env, &state)
}

/// Copies or cuts the selection; returns its plain text (empty if none).
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_copy(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    cut: jboolean,
) -> jstring {
    let view = &mut app(handle).view;
    let copied = if cut != 0 { view.cut() } else { view.copy() };
    let text = copied.map(|(text, _)| text).unwrap_or_default();
    new_string(&mut env, &text)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_paste(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    text: JString,
) {
    let text = string(&mut env, &text);
    app(handle).view.paste_text(&text);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_setDark(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    dark: jboolean,
) {
    app(handle).view.set_theme(if dark != 0 {
        Theme::dark()
    } else {
        Theme::light()
    });
}
