//! The C ABI used by `web/openrich.js`.
//!
//! Every function takes the `App` pointer returned by [`orx_init`]. Skia
//! objects are not thread-safe, so the module must stay on one thread (the
//! page's main thread).

use openrich_commands::{KeyEvent, KeyPlatform, Modifiers};
use openrich_extensions::EditorOptions;
use openrich_theme::Theme;
use openrich_view::{CursorIcon, EditorView, Effect, PointerButton, PointerEvent, ViewOptions};
use serde_json::{Value, json};
use skia_safe::gpu::{self, DirectContext, SurfaceOrigin, gl::FramebufferInfo};
use skia_safe::{ColorType, Surface};

pub struct App {
    view: EditorView,
    context: DirectContext,
    surface: Option<Surface>,
    /// Canvas size in device pixels.
    pixels: (i32, i32),
    /// Last string result, read by JavaScript through `orx_result`.
    result: Vec<u8>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App").field("pixels", &self.pixels).finish()
    }
}

/// Reads a UTF-8 string that JavaScript wrote into wasm memory.
///
/// # Safety
/// `ptr..ptr + len` must be readable (a buffer from [`orx_alloc`]).
unsafe fn read(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// # Safety
/// `app` must come from [`orx_init`] and not be destroyed.
unsafe fn app<'a>(app: *mut App) -> &'a mut App {
    unsafe { &mut *app }
}

fn modifiers(bits: u32) -> Modifiers {
    Modifiers {
        shift: bits & 1 != 0,
        ctrl: bits & 2 != 0,
        alt: bits & 4 != 0,
        meta: bits & 8 != 0,
    }
}

impl App {
    fn set_result(&mut self, text: &str) -> usize {
        self.result.clear();
        self.result.extend_from_slice(text.as_bytes());
        self.result.len()
    }

    fn pixels_for(width: f32, height: f32, dpr: f32) -> (i32, i32) {
        (
            (width * dpr).round().max(1.0) as i32,
            (height * dpr).round().max(1.0) as i32,
        )
    }
}

/// A buffer for JavaScript to write a string into.
#[unsafe(no_mangle)]
pub extern "C" fn orx_alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len.max(1));
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// # Safety
/// `ptr` and `len` must come from one [`orx_alloc`] call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_free(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len.max(1)) });
}

/// Pointer to the last string result (its length was returned by the call).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_result(app_ptr: *mut App) -> *const u8 {
    unsafe { app(app_ptr) }.result.as_ptr()
}

/// Creates the editor in the current WebGL context. `platform` is 1 on
/// Apple systems, 2 on Windows, 0 elsewhere. Returns null on failure.
///
/// # Safety
/// `content` must point to `content_len` bytes of UTF-8 JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_init(
    width: f32,
    height: f32,
    dpr: f32,
    platform: u32,
    dark: u32,
    content: *const u8,
    content_len: usize,
) -> *mut App {
    let content: Option<Value> = serde_json::from_str(&unsafe { read(content, content_len) }).ok();
    let Some(interface) = gpu::gl::Interface::new_native() else {
        return std::ptr::null_mut();
    };
    let Some(context) = gpu::direct_contexts::make_gl(interface, None) else {
        return std::ptr::null_mut();
    };
    let platform = match platform {
        1 => KeyPlatform::Apple,
        2 => KeyPlatform::Windows,
        _ => KeyPlatform::Other,
    };
    let Ok(mut view) = EditorView::new(ViewOptions {
        editor: EditorOptions {
            content,
            platform,
            ..Default::default()
        },
        theme: if dark != 0 {
            Theme::dark()
        } else {
            Theme::light()
        },
        width,
        height,
        scale: dpr,
        // The web has no system font manager; the bundle covers Latin,
        // Arabic and Devanagari (other scripts load lazily in P3).
        system_fonts: false,
        ..Default::default()
    }) else {
        return std::ptr::null_mut();
    };
    view.set_padding(openrich_view::Insets {
        top: 40.0,
        right: 24.0,
        bottom: 40.0,
        left: 24.0,
    });
    Box::into_raw(Box::new(App {
        view,
        context,
        surface: None,
        pixels: App::pixels_for(width, height, dpr),
        result: Vec::new(),
    }))
}

/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_resize(app_ptr: *mut App, width: f32, height: f32, dpr: f32) {
    let app = unsafe { app(app_ptr) };
    app.view.resize(width, height, dpr);
    let pixels = App::pixels_for(width, height, dpr);
    if pixels != app.pixels {
        app.pixels = pixels;
        app.surface = None;
    }
}

/// Paints a frame. Returns milliseconds until the next frame is needed
/// (0 while animating), or -1 when the view is idle.
///
/// # Safety
/// `app` must be live and its WebGL context current.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_frame(app_ptr: *mut App) -> i32 {
    let app = unsafe { app(app_ptr) };
    if app.surface.is_none() {
        let target = gpu::backend_render_targets::make_gl(
            app.pixels,
            0,
            8,
            FramebufferInfo {
                fboid: 0,
                format: gpu::gl::Format::RGBA8.into(),
                protected: gpu::Protected::No,
            },
        );
        app.surface = gpu::surfaces::wrap_backend_render_target(
            &mut app.context,
            &target,
            SurfaceOrigin::BottomLeft,
            ColorType::RGBA8888,
            None,
            None,
        );
    }
    let Some(surface) = app.surface.as_mut() else {
        return -1;
    };
    app.view.paint(surface.canvas());
    app.context.flush_and_submit();
    app.view
        .next_frame_in()
        .map_or(-1, |ms| i32::try_from(ms).unwrap_or(i32::MAX))
}

/// `kind`: 0 down, 1 move, 2 up. Coordinates in CSS pixels.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_pointer(
    app_ptr: *mut App,
    kind: u32,
    x: f32,
    y: f32,
    clicks: u32,
    mods: u32,
) -> u32 {
    let app = unsafe { app(app_ptr) };
    let event = PointerEvent {
        x,
        y,
        button: PointerButton::Primary,
        modifiers: modifiers(mods),
        click_count: clicks.max(1),
    };
    u32::from(match kind {
        0 => app.view.pointer_down(event),
        1 => app.view.pointer_move(event),
        _ => app.view.pointer_up(event),
    })
}

/// `smooth` animates (mouse wheels); otherwise the offset moves at once
/// (trackpads, which bring their own momentum).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_wheel(app_ptr: *mut App, dy: f32, smooth: u32) {
    let app = unsafe { app(app_ptr) };
    if smooth != 0 {
        app.view.scroll_smooth_by(dy);
    } else {
        app.view.scroll_by(dy);
    }
}

/// Starts momentum scrolling after a touch drag, in CSS px/s (positive
/// scrolls toward the end).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_fling(app_ptr: *mut App, velocity: f32) {
    unsafe { app(app_ptr) }.view.fling(velocity);
}

/// Stops scroll animations (a finger touching down).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_stop_scrolling(app_ptr: *mut App) {
    unsafe { app(app_ptr) }.view.stop_scrolling();
}

/// A `keydown`; returns 1 when the editor used it (call `preventDefault`).
///
/// # Safety
/// `app` must be live; the strings must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_key(
    app_ptr: *mut App,
    key: *const u8,
    key_len: usize,
    code: *const u8,
    code_len: usize,
    mods: u32,
) -> u32 {
    let app = unsafe { app(app_ptr) };
    let code = unsafe { read(code, code_len) };
    let event = KeyEvent {
        key: unsafe { read(key, key_len) },
        code: (!code.is_empty()).then_some(code),
        modifiers: modifiers(mods),
    };
    u32::from(app.view.key_down(&event))
}

/// Typed text outside a composition.
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_insert_text(app_ptr: *mut App, text: *const u8, len: usize) -> u32 {
    let app = unsafe { app(app_ptr) };
    u32::from(app.view.insert_text(&unsafe { read(text, len) }))
}

/// Replaces `from..to` (UTF-16, relative to the IME block) with `text`;
/// `composing` makes it the composition (EditContext `textupdate`).
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_ime_replace(
    app_ptr: *mut App,
    from: u32,
    to: u32,
    text: *const u8,
    len: usize,
    composing: u32,
) -> u32 {
    let app = unsafe { app(app_ptr) };
    let text = unsafe { read(text, len) };
    u32::from(
        app.view
            .ime_replace(from as usize, to as usize, &text, composing != 0),
    )
}

/// Sets the composition text (textarea fallback: `compositionupdate`).
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_set_composition(app_ptr: *mut App, text: *const u8, len: usize) {
    let app = unsafe { app(app_ptr) };
    app.view.set_composition(&unsafe { read(text, len) });
}

/// Commits the composition with `text` (`compositionend`).
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_commit_composition(app_ptr: *mut App, text: *const u8, len: usize) {
    let app = unsafe { app(app_ptr) };
    app.view.commit_composition(&unsafe { read(text, len) });
}

/// Ends the composition keeping its text (EditContext `compositionend`).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_finish_composition(app_ptr: *mut App) {
    unsafe { app(app_ptr) }.view.ime_finish_composition();
}

/// Sets the selection inside the IME block (EditContext `selectionchange`
/// is driven by the editor, but IMEs may move it).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_ime_select(app_ptr: *mut App, anchor: u32, head: u32) {
    unsafe { app(app_ptr) }
        .view
        .ime_set_selection(anchor as usize, head as usize);
}

/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_focus(app_ptr: *mut App, focused: u32) {
    unsafe { app(app_ptr) }.view.set_focused(focused != 0);
}

/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_set_dark(app_ptr: *mut App, dark: u32) {
    let app = unsafe { app(app_ptr) };
    app.view.set_theme(if dark != 0 {
        Theme::dark()
    } else {
        Theme::light()
    });
}

/// The IME block as JSON: `{start, text, selection, composition, caret}`
/// with UTF-16 offsets and the caret rectangle in CSS pixels. Returns the
/// result length.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_ime_state(app_ptr: *mut App) -> usize {
    let app = unsafe { app(app_ptr) };
    let caret = app.view.caret_rect().map(|r| [r.x, r.y, r.width, r.height]);
    let state = match app.view.ime_context() {
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
    };
    app.set_result(&state.to_string())
}

/// Effects since the last call, as a JSON array. Returns the result length.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_take_effects(app_ptr: *mut App) -> usize {
    let app = unsafe { app(app_ptr) };
    let effects: Vec<Value> = app
        .view
        .take_effects()
        .into_iter()
        .map(|effect| match effect {
            Effect::Redraw => json!({"type": "redraw"}),
            Effect::ImeCursorArea(r) => {
                json!({"type": "imeArea", "rect": [r.x, r.y, r.width, r.height]})
            }
            Effect::OpenUrl(url) => json!({"type": "openUrl", "url": url}),
            Effect::Cursor(icon) => json!({"type": "cursor", "cursor": match icon {
                CursorIcon::Text => "text",
                CursorIcon::Pointer => "pointer",
                CursorIcon::Default => "default",
            }}),
            Effect::Clipboard(content) => {
                json!({"type": "clipboard", "text": content.text, "html": content.html})
            }
            Effect::Haptic(_) => json!({"type": "haptic"}),
            Effect::SelectionMenu(menu) => json!({"type": "selectionMenu", "menu": menu.map(|m| json!({
                "rect": [m.rect.x, m.rect.y, m.rect.width, m.rect.height],
                "items": m.items.iter().map(|i| format!("{i:?}").to_lowercase()).collect::<Vec<_>>(),
            }))}),
            Effect::Magnifier(at) => json!({"type": "magnifier", "at": at.map(|(x, y)| [x, y])}),
            Effect::ShowKeyboard => json!({"type": "showKeyboard"}),
        })
        .collect();
    app.set_result(&Value::Array(effects).to_string())
}

/// Copies (or cuts) the selection. The result is JSON `{text, html}`
/// (empty strings when nothing is selected); returns its length.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_copy(app_ptr: *mut App, cut: u32) -> usize {
    let app = unsafe { app(app_ptr) };
    let copied = if cut != 0 {
        app.view.cut()
    } else {
        app.view.copy()
    };
    let result = match copied {
        Some(content) => json!({"text": content.text, "html": content.html}),
        None => json!({"text": "", "html": ""}),
    };
    app.set_result(&result.to_string())
}

/// Pastes plain text; each line becomes a paragraph.
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_paste(app_ptr: *mut App, text: *const u8, len: usize) -> u32 {
    let app = unsafe { app(app_ptr) };
    u32::from(app.view.paste_text(&unsafe { read(text, len) }))
}

/// Pastes clipboard HTML (formatting kept; exact for OpenRich's own copies).
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orx_paste_html(app_ptr: *mut App, html: *const u8, len: usize) -> u32 {
    let app = unsafe { app(app_ptr) };
    u32::from(app.view.paste_html(&unsafe { read(html, len) }))
}
