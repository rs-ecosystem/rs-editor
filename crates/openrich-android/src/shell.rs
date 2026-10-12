//! JNI entry points for `dev.openrich.android.OpenRichNative`.
//!
//! Every method takes the handle returned by `create` and must be called on
//! the UI thread (Skia objects are not thread-safe).

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeId, TreeUpdate};
use accesskit_android::InjectingAdapter;
use jni::objects::{GlobalRef, JClass, JObject, JString};
use jni::sys::{jboolean, jfloat, jint, jlong, jstring};
use jni::{JNIEnv, JavaVM};
use khronos_egl as egl;
use ndk::native_window::NativeWindow;
use openrich_commands::{KeyEvent, KeyPlatform, Modifiers};
use openrich_extensions::EditorOptions;
use openrich_theme::Theme;
use openrich_view::{EditorView, Effect, HapticKind, MenuItem, TouchPhase, ViewOptions};
use serde_json::{Value, json};
use skia_safe::gpu::{self, DirectContext, SurfaceOrigin, gl::FramebufferInfo};
use skia_safe::{ColorType, Surface};

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

pub struct App {
    view: EditorView,
    gl: Option<Gl>,
    /// TalkBack and other accessibility services (AccessKit).
    accessibility: Option<InjectingAdapter>,
    /// The Java view, to tell it about changes made by accessibility actions.
    host: Option<(JavaVM, GlobalRef)>,
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
        if !app.view.accessibility_action(&request) {
            return;
        }
        // Let the Java view redraw and resync the IME.
        if let Some((vm, host)) = &app.host
            && let Ok(mut env) = vm.attach_current_thread()
        {
            let _ = env.call_method(host.as_obj(), "afterInput", "()V", &[]);
        }
    }
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

    /// Effects for Java, with coordinates in pixels.
    fn effects(&mut self) -> Value {
        let scale = self.view.size().2;
        let effects: Vec<Value> = self
            .view
            .take_effects()
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Haptic(kind) => Some(json!({"type": "haptic", "kind": match kind {
                    HapticKind::LongPress => "longPress",
                    HapticKind::TextHandleMove => "textHandleMove",
                }})),
                Effect::SelectionMenu(menu) => Some(json!({"type": "menu", "menu": menu.map(|m| json!({
                    "rect": [m.rect.x * scale, m.rect.y * scale, m.rect.right() * scale, m.rect.bottom() * scale],
                    "items": m.items.iter().map(|item| menu_item_name(*item)).collect::<Vec<_>>(),
                }))})),
                Effect::Magnifier(at) => Some(json!({"type": "magnifier", "at": at.map(|(x, y)| [x * scale, y * scale])})),
                Effect::ShowKeyboard => Some(json!({"type": "showKeyboard"})),
                Effect::Clipboard(content) => Some(json!({"type": "clipboard", "text": content.text, "html": content.html})),
                Effect::OpenUrl(url) => Some(json!({"type": "openUrl", "url": url})),
                Effect::Redraw | Effect::ImeCursorArea(_) | Effect::Cursor(_) => None,
            })
            .collect();
        Value::Array(effects)
    }
}

fn menu_item_name(item: MenuItem) -> &'static str {
    match item {
        MenuItem::Cut => "cut",
        MenuItem::Copy => "copy",
        MenuItem::Paste => "paste",
        MenuItem::SelectAll => "selectAll",
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
        accessibility: None,
        host: None,
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
    // Changes since the last frame, while an accessibility service listens.
    if let Some(adapter) = app.accessibility.as_mut() {
        let view = &mut app.view;
        adapter.update_if_active(|| {
            view.accessibility_update().unwrap_or(TreeUpdate {
                nodes: Vec::new(),
                tree: None,
                tree_id: TreeId::ROOT,
                focus: openrich_view::EDITOR_NODE,
            })
        });
    }
    if resizing {
        return 0;
    }
    app.view.next_frame_in().map_or(-1, |ms| ms as jlong)
}

/// A touch event in pixels; `action` is 0 down, 1 move, 2 up, 3 cancel.
/// Results (keyboard, menu, haptics) come through `takeEffects`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_touch(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    action: jint,
    x: jfloat,
    y: jfloat,
) {
    let app = app(handle);
    let scale = app.view.size().2;
    let phase = match action {
        0 => TouchPhase::Down,
        1 => TouchPhase::Move,
        2 => TouchPhase::Up,
        _ => TouchPhase::Cancel,
    };
    app.view.touch(phase, x / scale, y / scale);
}

/// Effects since the last call as JSON: haptics, the selection menu,
/// the magnifier, keyboard requests, clipboard writes.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_takeEffects(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let effects = app(handle).effects().to_string();
    new_string(&mut env, &effects)
}

/// A command chosen from the selection menu (`cut`, `copy`, `paste`,
/// `selectAll`). For paste, `text` and `html` are the clipboard contents
/// (either may be null; HTML wins). Returns `{text, html}` JSON to put on
/// the clipboard, or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_menuAction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    item: JString,
    text: JString,
    html: JString,
) -> jstring {
    let item = match string(&mut env, &item).as_str() {
        "cut" => MenuItem::Cut,
        "copy" => MenuItem::Copy,
        "paste" => MenuItem::Paste,
        _ => MenuItem::SelectAll,
    };
    let optional =
        |env: &mut JNIEnv, value: &JString| (!value.is_null()).then(|| string(env, value));
    let text = optional(&mut env, &text);
    let html = optional(&mut env, &html);
    match app(handle)
        .view
        .menu_action(item, text.as_deref(), html.as_deref())
    {
        Some(content) => {
            let result = json!({"text": content.text, "html": content.html}).to_string();
            new_string(&mut env, &result)
        }
        None => std::ptr::null_mut(),
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

/// Copies or cuts the selection; returns `{text, html}` JSON (empty
/// strings when nothing is selected).
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_copy(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    cut: jboolean,
) -> jstring {
    let view = &mut app(handle).view;
    let copied = if cut != 0 { view.cut() } else { view.copy() };
    let result = match copied {
        Some(content) => json!({"text": content.text, "html": content.html}),
        None => json!({"text": "", "html": ""}),
    };
    new_string(&mut env, &result.to_string())
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

/// Pastes clipboard HTML; returns whether anything was pasted.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_pasteHtml(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    html: JString,
) -> jboolean {
    let html = string(&mut env, &html);
    jboolean::from(app(handle).view.paste_html(&html))
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

/// The system closed the selection menu (back button, a tap elsewhere).
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_menuDismissed(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    app(handle).view.selection_menu_dismissed();
}

/// `replaceText` (API 33): replaces `start..end` of the IME block with
/// `text`, as keyboards do to undo an autocorrection.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_replaceText(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    start: jint,
    end: jint,
    text: JString,
) {
    let text = string(&mut env, &text);
    app(handle)
        .view
        .ime_replace(start.max(0) as usize, end.max(0) as usize, &text, false);
}

/// Geometry for `CursorAnchorInfo`, in pixels: the caret line
/// (`[x, top, baseline, bottom]`) and the bounds of each UTF-16 unit in
/// `from..to` of the IME block (`null` where not laid out).
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_imeCursorInfo(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    from: jint,
    to: jint,
) -> jstring {
    let view = &mut app(handle).view;
    let scale = view.size().2;
    let caret = view.ime_caret().map(|c| {
        [
            c.x * scale,
            c.top * scale,
            c.baseline * scale,
            c.bottom * scale,
        ]
    });
    let bounds: Vec<Value> = view
        .ime_character_bounds(from.max(0) as usize, to.max(from).max(0) as usize)
        .into_iter()
        .map(|rect| {
            rect.map_or(Value::Null, |r| {
                json!([
                    r.x * scale,
                    r.y * scale,
                    r.right() * scale,
                    r.bottom() * scale
                ])
            })
        })
        .collect();
    let info = json!({"caret": caret, "bounds": bounds}).to_string();
    new_string(&mut env, &info)
}

/// Makes the editor visible to TalkBack: AccessKit installs itself as the
/// view's accessibility delegate.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_openrich_android_OpenRichNative_attachAccessibility(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    view: JObject,
) {
    let app = app(handle);
    if app.accessibility.is_some() {
        return;
    }
    if let (Ok(vm), Ok(host)) = (env.get_java_vm(), env.new_global_ref(&view)) {
        app.host = Some((vm, host));
    }
    app.accessibility = Some(InjectingAdapter::new(
        &mut env,
        &view,
        Accessibility(handle),
        Accessibility(handle),
    ));
}
