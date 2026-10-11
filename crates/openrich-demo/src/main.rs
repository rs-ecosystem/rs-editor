use openrich_document::{Document, Mark, Position, Selection};
use openrich_editor::{EditorCommand, EditorState, HorizontalDirection};
use openrich_layout::{ParagraphLayout, Point};
use openrich_render_skia::SkiaDocumentLayout;
use serde::Deserialize;
use serde_json::{Value, json};
use skia_safe::{Color, EncodedImageFormat, surfaces};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const INPUT_JS: &str = include_str!("../web/input.mjs");
const INTERACTION_JS: &str = include_str!("../web/interaction.mjs");
const APP_CSS: &str = include_str!("../web/style.css");
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

struct AppState {
    editor: EditorState,
}

#[derive(Deserialize)]
struct ActionRequest {
    command: String,
    expected_document: Option<Value>,
    #[serde(default)]
    text: String,
    mark: Option<String>,
    format: Option<openrich_document::BlockFormat>,
    selection: Option<Selection>,
    #[serde(default)]
    extend: bool,
}

fn main() -> std::io::Result<()> {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:4173".into());
    let listener = TcpListener::bind(&address)?;
    let state = Arc::new(Mutex::new(AppState {
        editor: EditorState::new(demo_document()).expect("demo fixture is valid"),
    }));
    println!("OpenRich interactive demo: http://{address}");
    println!("Press Ctrl+C to stop the local demo server.");
    // Persistent, bounded workers keep Skia's thread-local font caches warm.
    // Raster/encoding never holds the editor lock, so input remains responsive.
    let (sender, receiver) = mpsc::sync_channel::<TcpStream>(32);
    let receiver = Arc::new(Mutex::new(receiver));
    for _ in 0..4 {
        let receiver = Arc::clone(&receiver);
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            loop {
                let stream = { receiver.lock().unwrap().recv() };
                let Ok(stream) = stream else { break };
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(5)));
                if let Err(error) = handle_connection(stream, Arc::clone(&state)) {
                    eprintln!("request failed: {error}");
                }
            }
        });
    }
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if sender.send(stream).is_err() {
                    break;
                }
            }
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }
    Ok(())
}

fn demo_document() -> Document {
    Document::from_json(
        r#"{
          "schema_version": 1,
          "blocks": [
            {"type":"heading","level":1,"content":[{"type":"text","text":"Selamat datang di OpenRich","marks":[]}]},
            {"type":"paragraph","content":[{"type":"text","text":"Pilih teks untuk memformatnya. Ketik, buat paragraf dengan Enter, lalu coba undo dan redo.","marks":[]}]},
            {"type":"paragraph","content":[{"type":"text","text":"Unicode: emoji 🙂👩‍💻 · العربية · עברית · हिन्दी","marks":[]}]}
          ]
        }"#,
    )
    .expect("demo fixture JSON is valid")
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<AppState>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or("/");
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            if key.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    if content_length > MAX_BODY_BYTES {
        return respond(
            &mut stream,
            413,
            "application/json",
            br#"{"error":"request body too large"}"#,
        );
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;

    match (method, path.split('?').next().unwrap_or(path)) {
        ("GET", "/") => respond(
            &mut stream,
            200,
            "text/html; charset=utf-8",
            INDEX_HTML.as_bytes(),
        ),
        ("GET", "/app.js") => respond(
            &mut stream,
            200,
            "text/javascript; charset=utf-8",
            APP_JS.as_bytes(),
        ),
        ("GET", "/style.css") => respond(
            &mut stream,
            200,
            "text/css; charset=utf-8",
            APP_CSS.as_bytes(),
        ),
        ("GET", "/input.mjs") => respond(
            &mut stream,
            200,
            "text/javascript; charset=utf-8",
            INPUT_JS.as_bytes(),
        ),
        ("GET", "/interaction.mjs") => respond(
            &mut stream,
            200,
            "text/javascript; charset=utf-8",
            INTERACTION_JS.as_bytes(),
        ),
        ("GET", font_path) if font_path.starts_with("/fonts/") => {
            match openrich_render_skia::BUNDLED_FONTS
                .iter()
                .find(|(name, _)| Some(*name) == font_path.strip_prefix("/fonts/"))
            {
                Some((_, bytes)) => respond(&mut stream, 200, "font/ttf", bytes),
                None => respond_json(&mut stream, 404, &json!({"error":"font not found"})),
            }
        }
        ("GET", "/api/scene") => render_scene(&mut stream, &state, path, None),
        ("POST", "/api/preview-scene") => {
            let document = match Document::from_json(std::str::from_utf8(&body).unwrap_or_default())
            {
                Ok(document) => document,
                Err(error) => {
                    return respond_json(&mut stream, 400, &json!({"error":error.to_string()}));
                }
            };
            render_scene(&mut stream, &state, path, Some(document))
        }
        ("GET", "/api/state") => {
            let state = state.lock().unwrap();
            respond_json(&mut stream, 200, &editor_json(&state.editor))
        }
        ("GET", "/api/render") => render_document(&mut stream, &state, path),
        ("POST", "/api/pointer") => apply_pointer(&mut stream, &state, &body),
        ("POST", "/api/action") => apply_action(&mut stream, &state, &body),
        ("POST", "/api/load") => load_document(&mut stream, &state, &body),
        _ => respond(
            &mut stream,
            404,
            "application/json",
            br#"{"error":"not found"}"#,
        ),
    }
}

#[derive(Deserialize)]
struct PointerRequest {
    x: f32,
    y: f32,
    width: f32,
    scale: f32,
    anchor: Option<Position>,
}

fn apply_pointer(
    stream: &mut TcpStream,
    state: &Arc<Mutex<AppState>>,
    body: &[u8],
) -> std::io::Result<()> {
    let request: PointerRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(error) => return respond_json(stream, 400, &json!({"error": error.to_string()})),
    };
    if ![request.x, request.y, request.width, request.scale]
        .iter()
        .all(|v| v.is_finite())
    {
        return respond_json(stream, 400, &json!({"error":"invalid pointer geometry"}));
    }
    let mut state = state.lock().unwrap();
    let scale = request.scale.clamp(1.0, 3.0);
    let width = (request.width.clamp(320.0, 3200.0) * scale).round();
    let layout = match SkiaDocumentLayout::new(
        state.editor.document(),
        width - 56.0 * scale,
        16.0 * scale,
    ) {
        Ok(layout) => layout,
        Err(error) => return respond_json(stream, 400, &json!({"error":error.to_string()})),
    };
    if let Some(hit) = layout.hit_test_nearest(
        request.x * scale - 28.0 * scale,
        request.y * scale - 24.0 * scale,
    ) {
        let focus = Position {
            block_index: hit.top_level_index,
            offset: hit.hit.grapheme_offset,
            affinity: hit.hit.affinity,
        };
        let anchor = request.anchor.unwrap_or_else(|| focus.clone());
        if let Err(error) = state.editor.set_selection(Selection { anchor, focus }) {
            return respond_json(stream, 400, &json!({"error":error.to_string()}));
        }
    }
    respond_json(stream, 200, &editor_json(&state.editor))
}

fn apply_action(
    stream: &mut TcpStream,
    state: &Arc<Mutex<AppState>>,
    body: &[u8],
) -> std::io::Result<()> {
    let request: ActionRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(error) => return respond_json(stream, 400, &json!({"error": error.to_string()})),
    };
    let mut state = state.lock().unwrap();
    if let Some(expected) = &request.expected_document {
        if *expected != serde_json::to_value(state.editor.document()).unwrap() {
            return respond_json(
                stream,
                409,
                &json!({"error":"Dokumen berubah di perangkat lain. Muat ulang sebelum melanjutkan."}),
            );
        }
    }
    if let Some(selection) = request
        .selection
        .clone()
        .filter(|_| request.command != "composition_replace")
    {
        if let Err(error) = state.editor.set_selection(selection) {
            return respond_json(stream, 400, &json!({"error": error.to_string()}));
        }
    }
    if request.command == "get_selected_text" {
        return match state.editor.selected_text() {
            Ok(text) => respond_json(stream, 200, &json!({"text": text})),
            Err(error) => respond_json(stream, 400, &json!({"error": error.to_string()})),
        };
    }
    let result = match request.command.as_str() {
        "set_selection" => Ok(None),
        "end_history_group" => {
            state.editor.end_history_group();
            Ok(None)
        }
        "insert_text" => state
            .editor
            .dispatch(EditorCommand::InsertText(request.text)),
        "delete_backward" => state.editor.dispatch(EditorCommand::DeleteBackward),
        "delete_forward" => state.editor.dispatch(EditorCommand::DeleteForward),
        "split_block" => state.editor.dispatch(EditorCommand::SplitBlock),
        "toggle_mark" => match parse_mark(request.mark.as_deref()) {
            Some(mark) => state.editor.dispatch(EditorCommand::ToggleMark(mark)),
            None => return respond_json(stream, 400, &json!({"error":"unknown mark"})),
        },
        "set_block_format" => match request.format {
            Some(format) => state.editor.dispatch(EditorCommand::SetBlockFormat(format)),
            None => return respond_json(stream, 400, &json!({"error":"missing block format"})),
        },
        "undo" => state.editor.dispatch(EditorCommand::Undo),
        "redo" => state.editor.dispatch(EditorCommand::Redo),
        "move_backward" => state
            .editor
            .move_caret(HorizontalDirection::Backward, request.extend)
            .map(|_| None),
        "move_forward" => state
            .editor
            .move_caret(HorizontalDirection::Forward, request.extend)
            .map(|_| None),
        "composition_begin" => {
            state.editor.begin_composition();
            Ok(None)
        }
        "composition_update" => {
            state.editor.update_composition(request.text);
            Ok(None)
        }
        "composition_commit" => state.editor.commit_composition(request.text),
        "composition_replace" => match request.selection {
            Some(selection) => state.editor.replace_composition(selection, request.text),
            None => {
                return respond_json(
                    stream,
                    400,
                    &json!({"error":"missing composition replacement selection"}),
                );
            }
        },
        "composition_cancel" => {
            state.editor.cancel_composition();
            Ok(None)
        }
        _ => return respond_json(stream, 400, &json!({"error":"unknown command"})),
    };
    if let Err(error) = result {
        return respond_json(stream, 400, &json!({"error": error.to_string()}));
    }
    respond_json(stream, 200, &editor_json(&state.editor))
}

fn load_document(
    stream: &mut TcpStream,
    state: &Arc<Mutex<AppState>>,
    body: &[u8],
) -> std::io::Result<()> {
    let document = match Document::from_json(std::str::from_utf8(body).unwrap_or_default()) {
        Ok(document) => document,
        Err(error) => return respond_json(stream, 400, &json!({"error": error.to_string()})),
    };
    let editor = match EditorState::new(document) {
        Ok(editor) => editor,
        Err(error) => return respond_json(stream, 400, &json!({"error": error.to_string()})),
    };
    let mut state = state.lock().unwrap();
    state.editor = editor;
    respond_json(stream, 200, &editor_json(&state.editor))
}

fn render_document(
    stream: &mut TcpStream,
    state: &Arc<Mutex<AppState>>,
    request_path: &str,
) -> std::io::Result<()> {
    let state = state.lock().unwrap();
    let scale = query_value(request_path, "scale")
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(1.0)
        .clamp(1.0, 3.0);
    let width = query_value(request_path, "width")
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(960.0)
        .clamp(320.0, 3200.0);
    let width = (width * scale).round();
    let padding = 28.0 * scale;
    let font_size = 16.0 * scale;
    let layout =
        match SkiaDocumentLayout::new(state.editor.document(), width - padding * 2.0, font_size) {
            Ok(layout) => layout,
            Err(error) => return respond_json(stream, 500, &json!({"error": error.to_string()})),
        };
    let height = (layout.size().height.ceil() as i32 + (padding * 2.0) as i32)
        .clamp((180.0 * scale) as i32, (6000.0 * scale) as i32);
    let Some(mut surface) = surfaces::raster_n32_premul((width as i32, height)) else {
        return respond_json(
            stream,
            500,
            &json!({"error":"could not create Skia surface"}),
        );
    };
    surface.canvas().clear(Color::WHITE);
    layout.paint(
        surface.canvas(),
        Point {
            x: padding,
            y: 24.0 * scale,
        },
        Some(state.editor.selection()),
        true,
    );
    #[allow(deprecated)]
    let Some(data) = surface
        .image_snapshot()
        .encode_to_data(EncodedImageFormat::PNG)
    else {
        return respond_json(
            stream,
            500,
            &json!({"error":"could not encode Skia output"}),
        );
    };
    respond(stream, 200, "image/png", data.as_bytes())
}

/// Text pixels and interaction geometry are produced from one immutable layout.
/// The mutex is released before shaping/rasterization so edits never wait on PNG
/// encoding. Selection is a client overlay and does not invalidate this scene.
fn render_scene(
    stream: &mut TcpStream,
    state: &Arc<Mutex<AppState>>,
    path: &str,
    preview: Option<Document>,
) -> std::io::Result<()> {
    let started = Instant::now();
    let document = preview.unwrap_or_else(|| state.lock().unwrap().editor.document().clone());
    let scale = query_value(path, "scale")
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(1.0)
        .clamp(1.0, 3.0);
    let logical_width = query_value(path, "width")
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(960.0)
        .clamp(320.0, 3200.0);
    let width = (logical_width * scale).round();
    let layout = match SkiaDocumentLayout::new(&document, width - 56.0 * scale, 16.0 * scale) {
        Ok(layout) => layout,
        Err(error) => return respond_json(stream, 500, &json!({"error":error.to_string()})),
    };
    let layout_done = Instant::now();
    let height = (layout.size().height.ceil() as i32 + (56.0 * scale) as i32)
        .clamp((180.0 * scale) as i32, (6000.0 * scale) as i32);
    let mut blocks = Vec::new();
    for entry in &layout.document_layout().entries {
        // Editing nested child nodes is outside this demo's current schema API.
        if entry.depth != 0 {
            continue;
        }
        let Some(paragraph) = &entry.layout else {
            continue;
        };
        let mut glyphs = Vec::new();
        for offset in 0..paragraph.text_index_map().grapheme_len() {
            if let Ok(rects) = paragraph.selection_rects(offset..offset + 1) {
                for rect in rects {
                    let epsilon = (rect.width() * 0.01).clamp(0.001, 0.1);
                    let y = rect.y() + rect.height() * 0.5;
                    let left = paragraph.hit_test(rect.left + epsilon, y).unwrap();
                    let right = paragraph.hit_test(rect.right - epsilon, y).unwrap();
                    glyphs.push(json!({
                        "x": (28.0 * scale + entry.x + rect.x()) / scale,
                        "y": (24.0 * scale + entry.y + rect.y()) / scale,
                        "w": rect.width() / scale, "h": rect.height() / scale,
                        "offset": offset, "left": left.grapheme_offset, "right": right.grapheme_offset,
                        "left_affinity": left.affinity, "right_affinity": right.affinity,
                    }));
                }
            }
        }
        let empty = paragraph.caret_rect(0).ok().flatten().map(|rect| {
            json!({
                "x": (28.0 * scale + entry.x + rect.x) / scale,
                "y": (24.0 * scale + entry.y + rect.y) / scale,
                "h": rect.height / scale,
            })
        });
        let checkbox = match &document.blocks[entry.top_level_index] {
            openrich_document::Block::Formatted { format, .. }
                if format.list == openrich_document::ListStyle::Checkbox =>
            {
                Some(
                    json!({"x":28.0 + entry.x / scale - 36.0, "y":24.0 + entry.y / scale - 7.0, "w":32.0, "h":36.0}),
                )
            }
            _ => None,
        };
        blocks.push(json!({"index":entry.top_level_index, "glyphs":glyphs, "empty":empty, "checkbox":checkbox}));
    }
    let geometry_done = Instant::now();
    // Draw directly in the browser's RGBA byte order. The opaque white
    // background makes premultiplied and straight-alpha bytes identical.
    let info = skia_safe::ImageInfo::new(
        (width as i32, height),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let Some(mut surface) = surfaces::raster(&info, width as usize * 4, None) else {
        return respond_json(
            stream,
            500,
            &json!({"error":"could not create Skia surface"}),
        );
    };
    surface.canvas().clear(Color::WHITE);
    layout.paint(
        surface.canvas(),
        Point {
            x: 28.0 * scale,
            y: 24.0 * scale,
        },
        None,
        false,
    );
    let raster_done = Instant::now();
    // Small loopback scenes travel as RGBA directly: no PNG compression or
    // image decode on the typing path. Bound bandwidth/memory for tall scenes.
    let pixel_bytes = width as usize * height as usize * 4;
    let (format, pixels) =
        if pixel_bytes <= 8 * 1024 * 1024 && query_value(path, "format") != Some("png") {
            let pixels = surface
                .peek_pixels()
                .and_then(|pixmap| pixmap.bytes().map(<[u8]>::to_vec));
            let Some(pixels) = pixels else {
                return respond_json(stream, 500, &json!({"error":"could not read Skia pixels"}));
            };
            ("rgba8", pixels)
        } else {
            let mut options = skia_safe::png_encoder::Options::default();
            options.z_lib_level = 1;
            options.filter_flags = skia_safe::png_encoder::FilterFlag::NONE;
            let png = surface
                .peek_pixels()
                .and_then(|pixmap| skia_safe::png_encoder::encode_pixmap(&pixmap, &options));
            let Some(png) = png else {
                return respond_json(
                    stream,
                    500,
                    &json!({"error":"could not encode Skia output"}),
                );
            };
            ("png", png.as_bytes().to_vec())
        };
    let encoded = Instant::now();
    let metadata = json!({
        "metrics": {
            "layout_ms": (layout_done - started).as_secs_f64() * 1000.0,
            "geometry_ms": (geometry_done - layout_done).as_secs_f64() * 1000.0,
            "raster_ms": (raster_done - geometry_done).as_secs_f64() * 1000.0,
            "transfer_ms": (encoded - raster_done).as_secs_f64() * 1000.0,
            "total_ms": (encoded - started).as_secs_f64() * 1000.0,
        },
        "format": format, "pixel_width":width as u32, "pixel_height":height,
        "width": width / scale, "height": height as f32 / scale, "blocks":blocks,
        "document":serde_json::from_str::<Value>(&document.to_json().unwrap()).unwrap(),
    });
    let metadata = serde_json::to_vec(&metadata)?;
    let mut body = Vec::with_capacity(4 + metadata.len() + pixels.len());
    body.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    body.extend_from_slice(&metadata);
    body.extend_from_slice(&pixels);
    respond(stream, 200, "application/vnd.openrich.scene", &body)
}

fn query_value<'a>(path: &'a str, key: &str) -> Option<&'a str> {
    path.split_once('?')?.1.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then_some(value)
    })
}

fn editor_json(editor: &EditorState) -> Value {
    let document = editor
        .document()
        .to_json()
        .unwrap_or_else(|_| "null".into());
    json!({
        "document": serde_json::from_str::<Value>(&document).unwrap_or(Value::Null),
        "selection": editor.selection(),
        "active_marks": editor.active_marks(),
        "insertion_marks": editor.insertion_marks(),
        "composition": editor.composition().map(|composition| composition.text.as_str()),
    })
}

fn parse_mark(mark: Option<&str>) -> Option<Mark> {
    match mark? {
        "bold" => Some(Mark::Bold),
        "italic" => Some(Mark::Italic),
        "underline" => Some(Mark::Underline),
        "strike" => Some(Mark::Strike),
        "code" => Some(Mark::Code),
        _ => None,
    }
}

fn respond_json(stream: &mut TcpStream, status: u16, value: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    respond(stream, status, "application/json; charset=utf-8", &body)
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        409 => "Conflict",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}
