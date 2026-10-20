//! Times typing on a long document: `cargo run -p rs-editor-desktop --release --example perf`.

use std::time::Instant;

use rs_editor_extensions::EditorOptions;
use rs_editor_view::{Affinity, EditorView, ViewOptions};
use skia_safe::surfaces;

#[allow(dead_code)]
#[path = "../src/sample.rs"]
mod sample;

const PERF_ITERS: usize = 50;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(10_000);
    let start = Instant::now();
    let mut view = EditorView::new(ViewOptions {
        editor: EditorOptions {
            content: Some(sample::large(n)),
            ..Default::default()
        },
        width: 820.0,
        height: 720.0,
        scale: 2.0,
        ..Default::default()
    })
    .expect("view");
    let mut surface = surfaces::raster_n32_premul((1640, 1440)).expect("surface");
    view.paint(surface.canvas());
    println!("{n} blok: buka + frame pertama {:?}", start.elapsed());

    for (label, pos) in [
        ("awal", 3usize),
        ("tengah", view.editor().doc().content().size() / 2),
    ] {
        view.select_text(pos, pos, Affinity::Downstream);
        view.scroll_into_view();
        view.paint(surface.canvas());
        let mut samples = Vec::new();
        for _ in 0..PERF_ITERS {
            let t = Instant::now();
            view.insert_text("a");
            view.paint(surface.canvas());
            samples.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "ketik di {label}: median {:.2} ms, p95 {:.2} ms",
            samples[samples.len() / 2],
            samples[samples.len() * 95 / 100]
        );
    }
}
