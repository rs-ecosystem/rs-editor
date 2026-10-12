//! Desktop shell for the OpenRich editor view.
//!
//! A winit window with a Skia raster canvas presented through softbuffer.
//! It forwards keys, text, IME composition, mouse and scroll to
//! [`EditorView`] and acts on its [`Effect`]s. Today it is the development
//! harness; the Windows/Linux shell (GPU, AccessKit) grows from it.
//!
//! ```text
//! cargo run -p openrich-desktop --release [-- --dark] [-- --large 10000]
//! ```
//!
//! F2 toggles light/dark.

mod sample;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use openrich_commands::KeyPlatform;
use openrich_extensions::EditorOptions;
use openrich_theme::Theme;
use openrich_view::{
    CursorIcon, EditorView, Effect, KeyEvent, Modifiers, PointerButton, PointerEvent, ViewOptions,
};
use serde_json::Value;
use skia_safe::{AlphaType, ColorType, ImageInfo, surfaces};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

const PLATFORM: KeyPlatform = if cfg!(target_vendor = "apple") {
    KeyPlatform::Apple
} else if cfg!(windows) {
    KeyPlatform::Windows
} else {
    KeyPlatform::Other
};

/// Clicks closer than this (time and distance) count as a multi-click.
const MULTI_CLICK: Duration = Duration::from_millis(400);
const MULTI_CLICK_SLOP: f32 = 4.0;
const LINE_HEIGHT: f32 = 40.0;

struct Args {
    dark: bool,
    large: Option<usize>,
}

impl Args {
    fn parse() -> Self {
        let mut args = Args {
            dark: false,
            large: None,
        };
        let mut iter = std::env::args().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--dark" => args.dark = true,
                "--large" => args.large = iter.next().and_then(|n| n.parse().ok()).or(Some(10_000)),
                other => eprintln!("argumen tidak dikenal: {other}"),
            }
        }
        args
    }
}

struct Click {
    at: Instant,
    x: f32,
    y: f32,
    count: u32,
}

struct Shell {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    view: EditorView,
    dark: bool,
    modifiers: Modifiers,
    cursor: (f32, f32),
    pressed: bool,
    last_click: Option<Click>,
    /// The IME cleared its preedit; a commit may follow in the same batch.
    preedit_cleared: bool,
    /// Rich copy of the last copied range, pasted when the clipboard text still matches.
    copied: Option<(String, Value)>,
    clipboard: Option<arboard::Clipboard>,
}

#[derive(Default)]
struct App {
    args: Option<Args>,
    shell: Option<Shell>,
}

impl App {
    fn create(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> Result<Shell, Box<dyn std::error::Error>> {
        let args = self.args.take().unwrap_or(Args {
            dark: false,
            large: None,
        });
        let attributes = Window::default_attributes()
            .with_title("OpenRich")
            .with_inner_size(LogicalSize::new(820.0, 720.0))
            .with_min_inner_size(LogicalSize::new(320.0, 240.0));
        let window = Rc::new(event_loop.create_window(attributes)?);
        window.set_ime_allowed(true);
        let context = softbuffer::Context::new(window.clone())?;
        let surface = softbuffer::Surface::new(&context, window.clone())?;
        let size = window.inner_size().to_logical::<f32>(window.scale_factor());
        let content = match args.large {
            Some(n) => sample::large(n),
            None => sample::document(),
        };
        let mut view = EditorView::new(ViewOptions {
            editor: EditorOptions {
                content: Some(content),
                platform: PLATFORM,
                ..Default::default()
            },
            theme: if args.dark {
                Theme::dark()
            } else {
                Theme::light()
            },
            width: size.width,
            height: size.height,
            scale: window.scale_factor() as f32,
            ..Default::default()
        })?;
        view.set_padding(openrich_view::Insets {
            top: 48.0,
            right: 56.0,
            bottom: 48.0,
            left: 56.0,
        });
        view.set_focused(true);
        let clipboard = arboard::Clipboard::new()
            .inspect_err(|e| eprintln!("clipboard tidak tersedia: {e}"))
            .ok();
        Ok(Shell {
            window,
            surface,
            view,
            dark: args.dark,
            modifiers: Modifiers::default(),
            cursor: (0.0, 0.0),
            pressed: false,
            last_click: None,
            preedit_cleared: false,
            copied: None,
            clipboard,
        })
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.shell.is_some() {
            return;
        }
        match self.create(event_loop) {
            Ok(shell) => {
                shell.window.request_redraw();
                self.shell = Some(shell);
            }
            Err(error) => {
                eprintln!("gagal membuka jendela: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        if shell.preedit_cleared && !matches!(event, WindowEvent::Ime(Ime::Commit(_))) {
            shell.settle_preedit();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => shell.resize(),
            WindowEvent::RedrawRequested => shell.redraw(),
            WindowEvent::Focused(focused) => shell.view.set_focused(focused),
            WindowEvent::ModifiersChanged(state) => shell.modifiers = modifiers(state.state()),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                shell.key(&event);
            }
            WindowEvent::Ime(ime) => shell.ime(ime),
            WindowEvent::CursorMoved { position, .. } => {
                let position: LogicalPosition<f32> =
                    position.to_logical(shell.window.scale_factor());
                shell.cursor = (position.x, position.y);
                let event = shell.pointer(1);
                shell.view.pointer_move(event);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if state == ElementState::Pressed {
                    shell.pressed = true;
                    let count = shell.click_count();
                    let event = shell.pointer(count);
                    shell.view.pointer_down(event);
                } else {
                    shell.pressed = false;
                    let event = shell.pointer(1);
                    shell.view.pointer_up(event);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * LINE_HEIGHT,
                    MouseScrollDelta::PixelDelta(PhysicalPosition { y, .. }) => {
                        -(y as f32) / shell.window.scale_factor() as f32
                    }
                };
                shell.view.scroll_by(dy);
                if shell.pressed {
                    let event = shell.pointer(1);
                    shell.view.pointer_move(event);
                }
            }
            _ => {}
        }
        shell.apply_effects();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        if shell.preedit_cleared {
            shell.settle_preedit();
            shell.apply_effects();
        }
        // Wake for the next caret blink.
        match shell.view.next_frame_in() {
            Some(ms) => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(ms),
                ));
            }
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        if let (winit::event::StartCause::ResumeTimeReached { .. }, Some(shell)) =
            (cause, self.shell.as_ref())
        {
            shell.window.request_redraw();
        }
    }
}

impl Shell {
    fn resize(&mut self) {
        let scale = self.window.scale_factor();
        let size = self.window.inner_size().to_logical::<f32>(scale);
        self.view.resize(size.width, size.height, scale as f32);
        self.window.request_redraw();
    }

    fn redraw(&mut self) {
        let size = self.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if let Err(error) = self.surface.resize(width, height) {
            eprintln!("resize gagal: {error}");
            return;
        }
        let mut buffer = match self.surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(error) => {
                eprintln!("buffer gagal: {error}");
                return;
            }
        };
        // softbuffer pixels are 0RGB words, which is BGRA in little-endian memory.
        let info = ImageInfo::new(
            (size.width as i32, size.height as i32),
            ColorType::BGRA8888,
            AlphaType::Opaque,
            None,
        );
        let bytes: &mut [u8] = bytemuck::cast_slice_mut(&mut buffer);
        let row_bytes = size.width as usize * 4;
        if let Some(mut canvas) = surfaces::wrap_pixels(&info, bytes, row_bytes, None) {
            self.view.paint(canvas.canvas());
        }
        self.window.pre_present_notify();
        if let Err(error) = buffer.present() {
            eprintln!("present gagal: {error}");
        }
    }

    fn pointer(&self, click_count: u32) -> PointerEvent {
        PointerEvent {
            x: self.cursor.0,
            y: self.cursor.1,
            button: PointerButton::Primary,
            modifiers: self.modifiers,
            click_count,
        }
    }

    fn click_count(&mut self) -> u32 {
        let now = Instant::now();
        let (x, y) = self.cursor;
        let count = match &self.last_click {
            Some(last)
                if now.duration_since(last.at) < MULTI_CLICK
                    && (last.x - x).abs() <= MULTI_CLICK_SLOP
                    && (last.y - y).abs() <= MULTI_CLICK_SLOP =>
            {
                last.count % 3 + 1
            }
            _ => 1,
        };
        self.last_click = Some(Click {
            at: now,
            x,
            y,
            count,
        });
        count
    }

    fn key(&mut self, event: &winit::event::KeyEvent) {
        let m = self.modifiers;
        let command = if PLATFORM == KeyPlatform::Apple {
            m.meta
        } else {
            m.ctrl
        };
        let Some(name) = key_name(&event.logical_key) else {
            return;
        };
        if name == "F2" {
            self.dark = !self.dark;
            self.view.set_theme(if self.dark {
                Theme::dark()
            } else {
                Theme::light()
            });
            return;
        }
        if command && !m.alt && self.clipboard_key(&name.to_lowercase()) {
            return;
        }
        let code = match event.physical_key {
            PhysicalKey::Code(code) => Some(format!("{code:?}")),
            PhysicalKey::Unidentified(_) => None,
        };
        let key = KeyEvent {
            key: name,
            code,
            modifiers: m,
        };
        if self.view.key_down(&key) {
            return;
        }
        // Printable text that no binding claimed. With IME enabled, winit
        // reports composed text through `Ime::Commit` instead.
        let typing = !(m.ctrl || (PLATFORM == KeyPlatform::Apple && m.meta));
        if typing
            && let Some(text) = &event.text
            && !text.chars().any(char::is_control)
        {
            self.view.insert_text(text);
        }
    }

    fn clipboard_key(&mut self, key: &str) -> bool {
        match key {
            "c" | "x" => {
                let content = if key == "c" {
                    self.view.copy()
                } else {
                    self.view.cut()
                };
                if let Some((text, json)) = content {
                    if let Some(clipboard) = self.clipboard.as_mut() {
                        let _ = clipboard.set_text(text.clone());
                    }
                    self.copied = Some((text, json));
                }
                true
            }
            "v" => {
                let text = self.clipboard.as_mut().and_then(|c| c.get_text().ok());
                match (text, &self.copied) {
                    (Some(text), Some((copied, json))) if &text == copied => {
                        let json = json.clone();
                        self.view.paste_json(&json);
                    }
                    (Some(text), _) => {
                        self.view.paste_text(&text);
                    }
                    (None, Some((_, json))) => {
                        let json = json.clone();
                        self.view.paste_json(&json);
                    }
                    (None, None) => {}
                }
                true
            }
            _ => false,
        }
    }

    fn ime(&mut self, ime: Ime) {
        match ime {
            Ime::Enabled | Ime::Disabled => {}
            Ime::Preedit(text, _) if text.is_empty() => {
                // Either a commit follows or the composition was cancelled.
                self.preedit_cleared = self.view.composition_range().is_some();
            }
            Ime::Preedit(text, _) => {
                self.view.set_composition(&text);
            }
            Ime::Commit(text) => {
                self.preedit_cleared = false;
                self.view.commit_composition(&text);
            }
        }
    }

    fn settle_preedit(&mut self) {
        self.preedit_cleared = false;
        self.view.cancel_composition();
    }

    fn apply_effects(&mut self) {
        for effect in self.view.take_effects() {
            match effect {
                Effect::Redraw => self.window.request_redraw(),
                Effect::ImeCursorArea(rect) => self.window.set_ime_cursor_area(
                    LogicalPosition::new(rect.x, rect.y),
                    LogicalSize::new(rect.width.max(1.0), rect.height),
                ),
                Effect::Cursor(icon) => self.window.set_cursor(match icon {
                    CursorIcon::Text => winit::window::CursorIcon::Text,
                    CursorIcon::Pointer => winit::window::CursorIcon::Pointer,
                    CursorIcon::Default => winit::window::CursorIcon::Default,
                }),
                Effect::OpenUrl(url) => eprintln!("buka tautan: {url}"),
                Effect::Clipboard { text, json } => {
                    if let Some(clipboard) = self.clipboard.as_mut() {
                        let _ = clipboard.set_text(text.clone());
                    }
                    self.copied = Some((text, json));
                }
            }
        }
    }
}

fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        meta: state.super_key(),
    }
}

/// The W3C `KeyboardEvent.key` name of a winit key.
fn key_name(key: &Key) -> Option<String> {
    let name = match key {
        Key::Character(text) => return Some(text.to_string()),
        Key::Named(NamedKey::Space) => " ",
        Key::Named(NamedKey::Enter) => "Enter",
        Key::Named(NamedKey::Tab) => "Tab",
        Key::Named(NamedKey::Backspace) => "Backspace",
        Key::Named(NamedKey::Delete) => "Delete",
        Key::Named(NamedKey::Escape) => "Escape",
        Key::Named(NamedKey::ArrowLeft) => "ArrowLeft",
        Key::Named(NamedKey::ArrowRight) => "ArrowRight",
        Key::Named(NamedKey::ArrowUp) => "ArrowUp",
        Key::Named(NamedKey::ArrowDown) => "ArrowDown",
        Key::Named(NamedKey::Home) => "Home",
        Key::Named(NamedKey::End) => "End",
        Key::Named(NamedKey::PageUp) => "PageUp",
        Key::Named(NamedKey::PageDown) => "PageDown",
        Key::Named(NamedKey::F2) => "F2",
        _ => return None,
    };
    Some(name.to_owned())
}

fn main() {
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("event loop gagal: {error}");
            std::process::exit(1);
        }
    };
    let mut app = App {
        args: Some(Args::parse()),
        shell: None,
    };
    if let Err(error) = event_loop.run_app(&mut app) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
