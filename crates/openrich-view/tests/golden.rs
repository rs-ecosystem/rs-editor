//! Golden pixel tests: frames rendered with the bundled fonts only, compared
//! with PNGs in `tests/golden/` within a tolerance for anti-aliasing.
//!
//! `OPENRICH_UPDATE_GOLDEN=1 cargo test -p openrich-view --test golden`
//! rewrites the images. A mismatch writes the actual frame and a diff to
//! `target/golden-failures/`.

use std::path::{Path, PathBuf};

use openrich_commands::KeyPlatform;
use openrich_extensions::EditorOptions;
use openrich_state::ManualClock;
use openrich_theme::Theme;
use openrich_view::{Affinity, EditorView, ViewOptions};
use serde_json::{Value, json};
use skia_safe::{AlphaType, ColorType, Data, EncodedImageFormat, Image, ImageInfo, surfaces};

const WIDTH: f32 = 560.0;
const HEIGHT: f32 = 640.0;
const SCALE: f32 = 2.0;
/// A pixel differs when a channel is further apart than this.
const CHANNEL_TOLERANCE: u8 = 48;
/// Share of differing pixels a frame may have.
const PIXEL_TOLERANCE: f64 = 0.002;

fn text(text: &str, marks: &[&str]) -> Value {
    let marks: Vec<Value> = marks.iter().map(|m| json!({"type": m})).collect();
    if marks.is_empty() {
        json!({"type": "text", "text": text})
    } else {
        json!({"type": "text", "text": text, "marks": marks})
    }
}

fn p(content: Vec<Value>) -> Value {
    json!({"type": "paragraph", "content": content})
}

fn showcase() -> Value {
    json!({"type": "doc", "content": [
        {"type": "heading", "attrs": {"level": 1}, "content": [text("Catatan rapat", &[])]},
        p(vec![
            text("Teks ", &[]), text("tebal", &["bold"]), text(", ", &[]),
            text("miring", &["italic"]), text(", ", &[]), text("garis bawah", &["underline"]),
            text(", ", &[]), text("coret", &["strike"]), text(", ", &[]),
            text("kode", &["code"]), text(" dan ", &[]), text("sorotan", &["highlight"]), text(".", &[]),
        ]),
        {"type": "heading", "attrs": {"level": 2}, "content": [text("Daftar", &[])]},
        {"type": "bulletList", "content": [
            {"type": "listItem", "content": [p(vec![text("Satu", &[])]),
                {"type": "bulletList", "content": [{"type": "listItem", "content": [p(vec![text("Bertingkat", &[])])]}]}]},
            {"type": "listItem", "content": [p(vec![text("Dua", &[])])]},
        ]},
        {"type": "orderedList", "content": [
            {"type": "listItem", "content": [p(vec![text("Pertama", &[])])]},
            {"type": "listItem", "content": [p(vec![text("Kedua", &[])])]},
        ]},
        {"type": "taskList", "content": [
            {"type": "taskItem", "attrs": {"checked": true}, "content": [p(vec![text("Selesai", &[])])]},
            {"type": "taskItem", "attrs": {"checked": false}, "content": [p(vec![text("Belum", &[])])]},
        ]},
        {"type": "blockquote", "content": [p(vec![text("Kutipan yang cukup panjang sehingga baris kedua ikut terlihat di sini.", &[])])]},
        {"type": "codeBlock", "content": [text("fn main() {\n    println!(\"halo\");\n}", &[])]},
        {"type": "horizontalRule"},
        {"type": "paragraph"},
    ]})
}

fn scripts() -> Value {
    json!({"type": "doc", "content": [
        {"type": "heading", "attrs": {"level": 3}, "content": [text("Banyak aksara", &[])]},
        p(vec![text("العربية من اليمين إلى اليسار", &[])]),
        p(vec![text("हिन्दी देवनागरी लिपि", &[])]),
        p(vec![text("Campuran: kata ", &[]), text("عربي", &["bold"]), text(" dan ", &[]), text("हिन्दी", &["italic"]), text(".", &[])]),
    ]})
}

fn view(content: Value, theme: Theme) -> EditorView {
    let clock = ManualClock::new(1_000);
    let mut view = EditorView::new(ViewOptions {
        editor: EditorOptions {
            content: Some(content),
            platform: KeyPlatform::Other,
            clock: Some(clock),
            ..Default::default()
        },
        theme,
        width: WIDTH,
        height: HEIGHT,
        scale: SCALE,
        system_fonts: false,
        ..Default::default()
    })
    .expect("view");
    view.set_focused(true);
    view
}

fn render(view: &mut EditorView) -> Vec<u8> {
    let size = ((WIDTH * SCALE) as i32, (HEIGHT * SCALE) as i32);
    let mut surface = surfaces::raster_n32_premul(size).expect("surface");
    view.paint(surface.canvas());
    let image = surface.image_snapshot();
    let data = image
        .encode(None, EncodedImageFormat::PNG, None)
        .expect("encode");
    data.as_bytes().to_vec()
}

fn rgba(png: &[u8]) -> (i32, i32, Vec<u8>) {
    let image = Image::from_encoded(Data::new_copy(png)).expect("decode");
    let info = ImageInfo::new(
        (image.width(), image.height()),
        ColorType::RGBA8888,
        AlphaType::Unpremul,
        None,
    );
    let mut pixels = vec![0u8; info.compute_min_byte_size()];
    let row_bytes = info.min_row_bytes();
    assert!(image.read_pixels(
        &info,
        &mut pixels,
        row_bytes,
        (0, 0),
        skia_safe::image::CachingHint::Allow,
    ));
    (image.width(), image.height(), pixels)
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn check(name: &str, png: Vec<u8>) {
    let path = golden_dir().join(format!("{name}.png"));
    if std::env::var_os("OPENRICH_UPDATE_GOLDEN").is_some() || !path.exists() {
        std::fs::create_dir_all(golden_dir()).expect("golden dir");
        std::fs::write(&path, &png).expect("write golden");
        if std::env::var_os("OPENRICH_UPDATE_GOLDEN").is_none() {
            panic!("{name}: no golden image; wrote {}", path.display());
        }
        return;
    }
    let expected = std::fs::read(&path).expect("read golden");
    let (w, h, want) = rgba(&expected);
    let (aw, ah, got) = rgba(&png);
    assert_eq!((aw, ah), (w, h), "{name}: size changed");
    let mut diff = vec![0u8; want.len()];
    let mut differing = 0usize;
    let (want_pixels, got_pixels) = (want.as_chunks::<4>().0, got.as_chunks::<4>().0);
    for (i, (a, b)) in want_pixels.iter().zip(got_pixels).enumerate() {
        let off = a
            .iter()
            .zip(b)
            .take(3)
            .any(|(x, y)| x.abs_diff(*y) > CHANNEL_TOLERANCE);
        let d = &mut diff[i * 4..i * 4 + 4];
        if off {
            differing += 1;
            d.copy_from_slice(&[255, 0, 0, 255]);
        } else {
            // Faded expected image for context.
            let gray = (u16::from(a[0]) + u16::from(a[1]) + u16::from(a[2])) / 3;
            let faded = 200 + (gray / 5) as u8;
            d.copy_from_slice(&[faded, faded, faded, 255]);
        }
    }
    let share = differing as f64 / (w as f64 * h as f64);
    if share > PIXEL_TOLERANCE {
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/golden-failures");
        std::fs::create_dir_all(&out).expect("failure dir");
        std::fs::write(out.join(format!("{name}-actual.png")), &png).expect("write actual");
        let info = ImageInfo::new((w, h), ColorType::RGBA8888, AlphaType::Unpremul, None);
        let image =
            skia_safe::images::raster_from_data(&info, Data::new_copy(&diff), w as usize * 4)
                .expect("diff image");
        let encoded = image
            .encode(None, EncodedImageFormat::PNG, None)
            .expect("encode diff");
        std::fs::write(out.join(format!("{name}-diff.png")), encoded.as_bytes())
            .expect("write diff");
        panic!(
            "{name}: {:.3}% of pixels differ (allowed {:.3}%); see {}",
            share * 100.0,
            PIXEL_TOLERANCE * 100.0,
            out.display()
        );
    }
}

#[test]
fn showcase_light() {
    let mut view = view(showcase(), Theme::light());
    // Select across the marks paragraph.
    view.select_text(20, 48, Affinity::Downstream);
    check("showcase-light", render(&mut view));
}

#[test]
fn showcase_dark() {
    let mut view = view(showcase(), Theme::dark());
    view.select_text(20, 48, Affinity::Downstream);
    check("showcase-dark", render(&mut view));
}

#[test]
fn caret_and_placeholder() {
    let mut view = view(showcase(), Theme::light());
    let end = view.editor().doc().content().size() - 1;
    view.select_text(end, end, Affinity::Downstream);
    check("caret-placeholder", render(&mut view));
}

#[test]
fn bundled_scripts() {
    let mut view = view(scripts(), Theme::light());
    view.select_text(18, 24, Affinity::Downstream);
    check("scripts", render(&mut view));
}
