//! The C ABI used by the web package (`platforms/web`).
//!
//! Every function takes the `App` pointer returned by [`rse_init`]. Skia
//! objects are not thread-safe, so the module must stay on one thread (the
//! page's main thread). Coordinates are CSS pixels. Strings go in as UTF-8
//! written into buffers from [`rse_alloc`]; string results are left in the
//! app and read through [`rse_result`] (functions return their length).

use rs_editor_api::{ApiOptions, EditorApi, Units};
use rs_editor_commands::KeyPlatform;
use rs_editor_view::{Insets, InteractionProfile};
use skia_safe::gpu::{self, DirectContext, SurfaceOrigin, gl::FramebufferInfo};
use skia_safe::{ColorType, Surface};

pub struct App {
    api: EditorApi,
    context: DirectContext,
    surface: Option<Surface>,
    /// Canvas size in device pixels.
    pixels: (i32, i32),
    /// Last string result, read by JavaScript through `rse_result`.
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
/// `ptr..ptr + len` must be readable (a buffer from [`rse_alloc`]).
unsafe fn read(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Like [`read`], with a null pointer meaning "none".
///
/// # Safety
/// As for [`read`].
unsafe fn read_optional(ptr: *const u8, len: usize) -> Option<String> {
    (!ptr.is_null()).then(|| unsafe { read(ptr, len) })
}

/// # Safety
/// `app` must come from [`rse_init`] and not be destroyed.
unsafe fn app<'a>(app: *mut App) -> &'a mut App {
    unsafe { &mut *app }
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
pub extern "C" fn rse_alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len.max(1));
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// # Safety
/// `ptr` and `len` must come from one [`rse_alloc`] call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_free(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len.max(1)) });
}

/// Pointer to the last string result (its length was returned by the call).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_result(app_ptr: *mut App) -> *const u8 {
    unsafe { app(app_ptr) }.result.as_ptr()
}

/// Creates the editor in the current WebGL context. `platform` is 1 on
/// Apple systems, 2 on Windows, 0 elsewhere; `content` is a Tiptap JSON
/// document, HTML, or empty, or Markdown when `markdown` is 1. Returns
/// null on failure.
///
/// # Safety
/// `content` must point to `content_len` bytes of UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_init(
    width: f32,
    height: f32,
    dpr: f32,
    platform: u32,
    dark: u32,
    content: *const u8,
    content_len: usize,
    markdown: u32,
) -> *mut App {
    let Some(interface) = gpu::gl::Interface::new_native() else {
        return std::ptr::null_mut();
    };
    let Some(context) = gpu::direct_contexts::make_gl(interface, None) else {
        return std::ptr::null_mut();
    };
    let (platform, profile) = match platform {
        1 => (KeyPlatform::Apple, InteractionProfile::ios()),
        2 => (KeyPlatform::Windows, InteractionProfile::android()),
        _ => (KeyPlatform::Other, InteractionProfile::android()),
    };
    let api = EditorApi::new(ApiOptions {
        width,
        height,
        scale: dpr,
        units: Units::Logical,
        dark: dark != 0,
        content: unsafe { read(content, content_len) },
        markdown: markdown != 0,
        platform,
        // The web has no system font manager; the bundle covers Latin,
        // Arabic and Devanagari (other scripts load lazily).
        system_fonts: false,
        padding: Insets {
            top: 40.0,
            right: 24.0,
            bottom: 40.0,
            left: 24.0,
        },
        profile,
    });
    Box::into_raw(Box::new(App {
        api,
        context,
        surface: None,
        pixels: App::pixels_for(width, height, dpr),
        result: Vec::new(),
    }))
}

/// Frees the editor; `app` is dead afterwards.
///
/// # Safety
/// `app` must be live and its WebGL context current.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_destroy(app_ptr: *mut App) {
    if !app_ptr.is_null() {
        drop(unsafe { Box::from_raw(app_ptr) });
    }
}

/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_resize(app_ptr: *mut App, width: f32, height: f32, dpr: f32) {
    let app = unsafe { app(app_ptr) };
    app.api.resize(width, height, dpr);
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
pub unsafe extern "C" fn rse_frame(app_ptr: *mut App) -> i32 {
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
    app.api.view_mut().paint(surface.canvas());
    app.context.flush_and_submit();
    i32::try_from(app.api.next_frame_in()).unwrap_or(i32::MAX)
}

// --- pointer, touch and scrolling ---------------------------------------------

/// A mouse event: `kind` 0 down, 1 move, 2 up; `mods` bits 1 shift, 2 ctrl,
/// 4 alt, 8 meta. Returns 1 when the editor used it.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_pointer(
    app_ptr: *mut App,
    kind: u32,
    x: f32,
    y: f32,
    clicks: u32,
    mods: u32,
) -> u32 {
    let app = unsafe { app(app_ptr) };
    u32::from(app.api.pointer(kind as i32, x, y, clicks, mods))
}

/// A touch (one finger): `action` 0 down, 1 move, 2 up, 3 cancel. Taps,
/// long presses, handles, scrolling and flings are recognized in Rust;
/// results come through [`rse_take_effects`].
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_touch(app_ptr: *mut App, action: u32, x: f32, y: f32) {
    unsafe { app(app_ptr) }.api.touch(action as i32, x, y);
}

/// `smooth` animates (mouse wheels); otherwise the offset moves at once
/// (trackpads, which bring their own momentum).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_wheel(app_ptr: *mut App, dy: f32, smooth: u32) {
    unsafe { app(app_ptr) }.api.wheel(dy, smooth != 0);
}

/// CSS pixels at the bottom covered by the on-screen keyboard (or the
/// app's UI over the editor).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_set_obscured_bottom(app_ptr: *mut App, height: f32) {
    unsafe { app(app_ptr) }.api.set_obscured_bottom(height);
}

/// Effects since the last call, as a JSON array; returns its length.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_take_effects(app_ptr: *mut App) -> usize {
    let app = unsafe { app(app_ptr) };
    let effects = app.api.take_effects();
    app.set_result(&effects)
}

/// A selection menu command (`cut`, `copy`, `paste`, `selectAll`); for
/// paste, the clipboard's text and HTML (null pointers when missing).
/// Leaves `{text, html}` JSON to put on the clipboard (or nothing).
///
/// # Safety
/// `app` must be live; the strings must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_menu_action(
    app_ptr: *mut App,
    item: *const u8,
    item_len: usize,
    text: *const u8,
    text_len: usize,
    html: *const u8,
    html_len: usize,
) -> usize {
    let app = unsafe { app(app_ptr) };
    let item = unsafe { read(item, item_len) };
    let text = unsafe { read_optional(text, text_len) };
    let html = unsafe { read_optional(html, html_len) };
    let result = app.api.menu_action(&item, text.as_deref(), html.as_deref());
    app.set_result(&result)
}

/// The menu was closed by something else (a tap outside, Escape).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_menu_dismissed(app_ptr: *mut App) {
    unsafe { app(app_ptr) }.api.menu_dismissed();
}

// --- keys and the input method ------------------------------------------------

/// A `keydown`; returns 1 when the editor used it (call `preventDefault`).
///
/// # Safety
/// `app` must be live; the strings must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_key(
    app_ptr: *mut App,
    key: *const u8,
    key_len: usize,
    code: *const u8,
    code_len: usize,
    mods: u32,
) -> u32 {
    let app = unsafe { app(app_ptr) };
    let key = unsafe { read(key, key_len) };
    let code = unsafe { read(code, code_len) };
    u32::from(app.api.key_down(&key, &code, mods))
}

/// Typed text outside a composition.
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_insert_text(app_ptr: *mut App, text: *const u8, len: usize) -> u32 {
    let app = unsafe { app(app_ptr) };
    u32::from(app.api.insert_text(&unsafe { read(text, len) }))
}

/// Replaces `from..to` (UTF-16, relative to the IME block) with `text`;
/// `composing` makes it the composition (EditContext `textupdate`).
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_ime_replace(
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
        app.api
            .replace_text(from as i32, to as i32, &text, composing != 0),
    )
}

/// Sets the composition text (textarea fallback: `compositionupdate`).
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_set_composition(app_ptr: *mut App, text: *const u8, len: usize) {
    let app = unsafe { app(app_ptr) };
    app.api.set_composing_text(&unsafe { read(text, len) });
}

/// Commits the composition with `text` (`compositionend`).
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_commit_composition(app_ptr: *mut App, text: *const u8, len: usize) {
    let app = unsafe { app(app_ptr) };
    app.api.commit_composition(&unsafe { read(text, len) });
}

/// Ends the composition keeping its text (EditContext `compositionend`).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_finish_composition(app_ptr: *mut App) {
    unsafe { app(app_ptr) }.api.finish_composing();
}

/// Sets the selection inside the IME block (IMEs may move it).
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_ime_select(app_ptr: *mut App, anchor: u32, head: u32) {
    unsafe { app(app_ptr) }
        .api
        .set_selection(anchor as i32, head as i32);
}

/// The IME block as JSON `{start, text, selection, composition, caret}`
/// (caret `[x, y, width, height]`); returns its length.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_ime_state(app_ptr: *mut App) -> usize {
    let app = unsafe { app(app_ptr) };
    let state = app.api.ime_state();
    app.set_result(&state)
}

/// Caret line and character bounds of `from..to` in the IME block, as JSON
/// (for EditContext `characterboundsupdate`); returns its length.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_ime_cursor_info(app_ptr: *mut App, from: u32, to: u32) -> usize {
    let app = unsafe { app(app_ptr) };
    let info = app.api.ime_cursor_info(from as i32, to as i32);
    app.set_result(&info)
}

/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_focus(app_ptr: *mut App, focused: u32) {
    unsafe { app(app_ptr) }.api.set_focused(focused != 0);
}

/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_set_dark(app_ptr: *mut App, dark: u32) {
    unsafe { app(app_ptr) }.api.set_dark(dark != 0);
}

// --- clipboard ----------------------------------------------------------------

/// Copies (or cuts) the selection; leaves `{text, html}` JSON (empty
/// strings when nothing is selected) and returns its length.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_copy(app_ptr: *mut App, cut: u32) -> usize {
    let app = unsafe { app(app_ptr) };
    let copied = app.api.copy(cut != 0);
    app.set_result(&copied)
}

/// Pastes plain text; each line becomes a paragraph.
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_paste(app_ptr: *mut App, text: *const u8, len: usize) -> u32 {
    let app = unsafe { app(app_ptr) };
    u32::from(app.api.paste(&unsafe { read(text, len) }))
}

/// Pastes clipboard HTML (formatting kept; exact for Rs Editor's own copies).
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_paste_html(app_ptr: *mut App, html: *const u8, len: usize) -> u32 {
    let app = unsafe { app(app_ptr) };
    u32::from(app.api.paste_html(&unsafe { read(html, len) }))
}

// --- content and commands (the Tiptap API) ---------------------------------------

/// `what`: 0 Tiptap JSON, 1 HTML, 2 plain text, 3 Markdown. Returns the
/// length.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_get_content(app_ptr: *mut App, what: u32) -> usize {
    let app = unsafe { app(app_ptr) };
    let content = match what {
        0 => app.api.get_json(),
        1 => app.api.get_html(),
        3 => app.api.get_markdown(),
        _ => app.api.get_text(),
    };
    app.set_result(&content)
}

/// Replaces the document with Tiptap JSON (`format` 0), HTML (1) or
/// Markdown (2); returns 1 when it fit the schema.
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_set_content(
    app_ptr: *mut App,
    content: *const u8,
    len: usize,
    format: u32,
) -> u32 {
    let app = unsafe { app(app_ptr) };
    let content = unsafe { read(content, len) };
    u32::from(match format {
        1 => app.api.set_html(&content),
        2 => app.api.set_markdown(&content),
        _ => app.api.set_json(&content),
    })
}

/// Adds a font from file bytes for characters the bundled fonts lack (see
/// the `missingCharacters` effect), under `family` (empty: the font's own
/// name). Leaves the family as the result and returns its length (0 when
/// the bytes are not a font).
///
/// # Safety
/// `app` must be live; the buffers must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_add_font(
    app_ptr: *mut App,
    bytes: *const u8,
    len: usize,
    family: *const u8,
    family_len: usize,
) -> usize {
    let app = unsafe { app(app_ptr) };
    let bytes = if bytes.is_null() || len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, len) }
    };
    let family = unsafe { read(family, family_len) };
    let added = app.api.add_font(bytes, &family);
    app.set_result(&added)
}

/// Whether pasted plain text is read as Markdown.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_set_markdown_paste(app_ptr: *mut App, markdown: u32) {
    unsafe { app(app_ptr) }
        .api
        .set_markdown_paste(markdown != 0);
}

/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_is_empty(app_ptr: *mut App) -> u32 {
    u32::from(unsafe { app(app_ptr) }.api.is_empty())
}

/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_set_editable(app_ptr: *mut App, editable: u32) {
    unsafe { app(app_ptr) }.api.set_editable(editable != 0);
}

/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_is_editable(app_ptr: *mut App) -> u32 {
    u32::from(unsafe { app(app_ptr) }.api.is_editable())
}

/// Runs (`dry_run` 0) or checks (`dry_run` 1) a chain of commands given as
/// `[{"name", "args"}, …]`; returns 1 when every command applies.
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_chain(
    app_ptr: *mut App,
    steps: *const u8,
    len: usize,
    dry_run: u32,
) -> u32 {
    let app = unsafe { app(app_ptr) };
    let steps = unsafe { read(steps, len) };
    u32::from(if dry_run != 0 {
        app.api.can_chain(&steps)
    } else {
        app.api.run_chain(&steps)
    })
}

/// Whether the mark or node `name` with attributes `attrs` (JSON, `{}`
/// for any) is active at the selection.
///
/// # Safety
/// `app` must be live; the strings must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_is_active(
    app_ptr: *mut App,
    name: *const u8,
    name_len: usize,
    attrs: *const u8,
    attrs_len: usize,
) -> u32 {
    let app = unsafe { app(app_ptr) };
    let name = unsafe { read(name, name_len) };
    let attrs = unsafe { read(attrs, attrs_len) };
    u32::from(app.api.is_active(&name, &attrs))
}

/// Attributes of the mark or node `name` at the selection, as JSON;
/// returns the length.
///
/// # Safety
/// `app` must be live; the string must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_get_attributes(
    app_ptr: *mut App,
    name: *const u8,
    len: usize,
) -> usize {
    let app = unsafe { app(app_ptr) };
    let attributes = app.api.get_attributes(&unsafe { read(name, len) });
    app.set_result(&attributes)
}

/// The selection as JSON `{anchor, head, from, to, empty}`; returns the
/// length.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_selection(app_ptr: *mut App) -> usize {
    let app = unsafe { app(app_ptr) };
    let selection = app.api.selection();
    app.set_result(&selection)
}

/// The accessibility tree as nested JSON (see
/// `EditorApi::accessibility_json`); returns the length.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_accessibility_tree(app_ptr: *mut App) -> usize {
    let app = unsafe { app(app_ptr) };
    let tree = app.api.accessibility_json();
    app.set_result(&tree)
}

/// An accessibility action (`click`, `focus`) on a node of that tree;
/// returns 1 when it applied.
///
/// # Safety
/// `app` must be live; the strings must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_accessibility_action(
    app_ptr: *mut App,
    id: *const u8,
    id_len: usize,
    action: *const u8,
    action_len: usize,
) -> u32 {
    let app = unsafe { app(app_ptr) };
    let id = unsafe { read(id, id_len) };
    let action = unsafe { read(action, action_len) };
    u32::from(app.api.accessibility_action(&id, &action))
}

/// Revisions (wrapping): `doc` 0 changes with any transaction, 1 only with
/// document changes.
///
/// # Safety
/// `app` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rse_revision(app_ptr: *mut App, doc: u32) -> u32 {
    let app = unsafe { app(app_ptr) };
    let revision = if doc != 0 {
        app.api.doc_revision()
    } else {
        app.api.revision()
    };
    revision as u32
}
