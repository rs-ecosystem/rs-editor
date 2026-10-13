use std::sync::Arc;

use rs_editor_commands::KeyPlatform;
use rs_editor_extensions::EditorOptions;
use rs_editor_state::{ManualClock, Selection};
use serde_json::{Value, json};

use crate::*;

fn view_with(content: Value, platform: KeyPlatform) -> (EditorView, Arc<ManualClock>) {
    let clock = ManualClock::new(10_000);
    let view = EditorView::new(ViewOptions {
        editor: EditorOptions {
            content: Some(content),
            platform,
            clock: Some(clock.clone()),
            ..Default::default()
        },
        width: 640.0,
        height: 400.0,
        system_fonts: false,
        ..Default::default()
    })
    .unwrap();
    (view, clock)
}

fn p(text: &str) -> Value {
    if text.is_empty() {
        json!({"type": "paragraph"})
    } else {
        json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
    }
}

fn doc(blocks: Vec<Value>) -> Value {
    json!({"type": "doc", "content": blocks})
}

fn key(view: &mut EditorView, name: &str, modifiers: Modifiers) -> bool {
    view.key_down(&KeyEvent::new(name, modifiers))
}

fn plain() -> Modifiers {
    Modifiers::default()
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Default::default()
    }
}

fn alt() -> Modifiers {
    Modifiers {
        alt: true,
        ..Default::default()
    }
}

fn cmd() -> Modifiers {
    Modifiers {
        meta: true,
        ..Default::default()
    }
}

fn text_of(view: &EditorView) -> String {
    view.editor().get_text("\n")
}

fn head(view: &EditorView) -> usize {
    view.editor().selection().head()
}

#[test]
fn typing_moves_the_caret_and_reports_the_ime_area() {
    let (mut view, _) = view_with(doc(vec![p("")]), KeyPlatform::Other);
    view.set_focused(true);
    let before = view.caret_rect().unwrap();
    assert!(view.insert_text("Halo"));
    let after = view.caret_rect().unwrap();
    assert!(after.x > before.x);
    let effects = view.take_effects();
    assert!(effects.contains(&Effect::Redraw));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::ImeCursorArea(_)))
    );
}

#[test]
fn arrows_step_over_whole_graphemes_and_shift_extends() {
    let (mut view, _) = view_with(doc(vec![p("a👩‍💻b")]), KeyPlatform::Other);
    view.select_text(1, 1, Affinity::Downstream);
    assert!(key(&mut view, "ArrowRight", plain()));
    assert_eq!(head(&view), 2);
    assert!(key(&mut view, "ArrowRight", plain()));
    assert_eq!(head(&view), 7, "the ZWJ sequence is one step");
    assert!(key(&mut view, "ArrowLeft", shift()));
    let selection = view.editor().selection().clone();
    assert_eq!((selection.anchor(), selection.head()), (7, 2));
    assert!(key(&mut view, "ArrowLeft", plain()));
    assert_eq!(
        (
            view.editor().selection().from(),
            view.editor().selection().to()
        ),
        (2, 2),
        "an arrow collapses a range"
    );
}

#[test]
fn up_and_down_keep_the_column_across_blocks() {
    let (mut view, _) = view_with(
        doc(vec![p("abcdefghij"), p("x"), p("ABCDEFGHIJ")]),
        KeyPlatform::Other,
    );
    view.select_text(7, 7, Affinity::Downstream);
    assert!(key(&mut view, "ArrowDown", plain()));
    assert_eq!(head(&view), 14, "a short line clamps to its end");
    assert!(key(&mut view, "ArrowDown", plain()));
    let third = head(&view);
    assert!(
        (20..=24).contains(&third),
        "the goal column is kept: {third}"
    );
    assert!(key(&mut view, "ArrowUp", plain()));
    assert!(key(&mut view, "ArrowUp", plain()));
    assert_eq!(head(&view), 7);
    assert!(key(&mut view, "ArrowUp", plain()));
    assert_eq!(head(&view), 1, "up on the first line goes to the start");
}

#[test]
fn backspace_deletes_graphemes_words_and_lines() {
    let (mut view, _) = view_with(doc(vec![p("satu dua 👩‍💻")]), KeyPlatform::Apple);
    let end = view.editor().doc().content().size() - 1;
    view.select_text(end, end, Affinity::Downstream);
    assert!(key(&mut view, "Backspace", plain()));
    assert_eq!(text_of(&view), "satu dua ");
    assert!(key(&mut view, "Backspace", alt()));
    assert_eq!(text_of(&view), "satu ");
    assert!(key(&mut view, "Backspace", cmd()));
    assert_eq!(text_of(&view), "");
}

#[test]
fn enter_and_backspace_use_the_extension_keymap() {
    let (mut view, _) = view_with(doc(vec![p("abcd")]), KeyPlatform::Other);
    view.select_text(3, 3, Affinity::Downstream);
    assert!(key(&mut view, "Enter", plain()));
    assert_eq!(text_of(&view), "ab\ncd");
    assert!(key(&mut view, "Backspace", plain()));
    assert_eq!(text_of(&view), "abcd");
}

#[test]
fn a_chain_runs_as_one_undo_step_and_bumps_the_revision() {
    let (mut view, _) = view_with(doc(vec![p("abcd")]), KeyPlatform::Other);
    view.select_text(1, 5, Affinity::Downstream);
    let steps = vec![
        ("toggleBold".to_string(), Value::Null),
        ("toggleHeading".to_string(), json!({"level": 2})),
    ];
    let before = view.revision();
    let doc_before = view.doc_revision();
    view.select_text(1, 3, Affinity::Downstream);
    assert!(view.revision() > before);
    assert_eq!(view.doc_revision(), doc_before, "a selection move");
    view.select_text(1, 5, Affinity::Downstream);
    let before = view.revision();
    assert!(view.can_chain(&steps).unwrap());
    assert_eq!(view.revision(), before, "a dry run changes nothing");
    assert!(view.run_chain(&steps).unwrap());
    assert!(view.revision() > before);
    assert!(view.doc_revision() > doc_before);
    let editor = view.editor();
    assert!(editor.is_active("bold", &Default::default()));
    assert!(editor.is_active("heading", json!({"level": 2}).as_object().unwrap()));
    assert!(view.run("undo", &Value::Null).unwrap());
    let editor = view.editor();
    assert!(!editor.is_active("bold", &Default::default()));
    assert!(!editor.is_active("heading", &Default::default()));
    assert!(
        view.run_chain(&[("noSuchCommand".to_string(), Value::Null)])
            .is_err()
    );
}

#[test]
fn a_keyboard_covering_the_caret_scrolls_it_into_view() {
    let blocks = (0..40).map(|i| p(&format!("baris {i}"))).collect();
    let (mut view, _) = view_with(doc(blocks), KeyPlatform::Other);
    view.set_focused(true);
    // A caret near the bottom of the 400px view.
    let mut pos = 1;
    loop {
        view.select_text(pos, pos, Affinity::Downstream);
        if view.caret_rect().unwrap().bottom() > 340.0 {
            break;
        }
        pos += 9;
    }
    assert_eq!(view.scroll_y, 0.0);
    // The keyboard slides in over the bottom half, in steps.
    for covered in [50.0, 120.0, 200.0] {
        view.set_obscured_bottom(covered);
        assert!(
            view.caret_rect().unwrap().bottom() <= 400.0 - covered,
            "{covered}"
        );
    }
    assert!(view.scroll_y > 0.0);
    assert_eq!(view.visible_height(), 200.0);
    // Scrolling reaches the end of the document above the keyboard.
    view.scroll_by(1e6);
    let max = view.scroll_y;
    view.set_obscured_bottom(0.0);
    assert!(view.scroll_y < max, "the end moves back down");
}

#[test]
fn clicks_select_positions_words_and_blocks() {
    let (mut view, _) = view_with(doc(vec![p("halo dunia indah")]), KeyPlatform::Other);
    let caret = {
        view.select_text(8, 8, Affinity::Downstream);
        view.caret_rect().unwrap()
    };
    view.select_text(1, 1, Affinity::Downstream);
    let y = caret.center_y();
    assert!(view.pointer_down(PointerEvent::at(caret.x, y)));
    view.pointer_up(PointerEvent::at(caret.x, y));
    assert_eq!(head(&view), 8);
    let double = PointerEvent {
        click_count: 2,
        ..PointerEvent::at(caret.x, y)
    };
    view.pointer_down(double);
    let selection = view.editor().selection().clone();
    assert_eq!(
        (selection.from(), selection.to()),
        (6, 11),
        "double click selects `dunia`"
    );
    view.pointer_up(double);
    let triple = PointerEvent {
        click_count: 3,
        ..PointerEvent::at(caret.x, y)
    };
    view.pointer_down(triple);
    let selection = view.editor().selection().clone();
    assert_eq!((selection.from(), selection.to()), (1, 17));
}

#[test]
fn dragging_extends_the_selection() {
    let (mut view, _) = view_with(doc(vec![p("satu"), p("dua")]), KeyPlatform::Other);
    view.select_text(2, 2, Affinity::Downstream);
    let start = view.caret_rect().unwrap();
    view.select_text(8, 8, Affinity::Downstream);
    let end = view.caret_rect().unwrap();
    view.pointer_down(PointerEvent::at(start.x, start.center_y()));
    assert!(view.pointer_move(PointerEvent::at(end.x, end.center_y())));
    view.pointer_up(PointerEvent::at(end.x, end.center_y()));
    let selection = view.editor().selection().clone();
    assert_eq!((selection.anchor(), selection.head()), (2, 8));
}

#[test]
fn clicking_a_checkbox_toggles_the_task() {
    let content = doc(vec![json!({"type": "taskList", "content": [
        {"type": "taskItem", "attrs": {"checked": false}, "content": [p("tugas")]}
    ]})]);
    let (mut view, _) = view_with(content, KeyPlatform::Other);
    let rect = view
        .layout()
        .decorations
        .iter()
        .find_map(|d| match d {
            rs_editor_layout::Decoration::Checkbox { rect, .. } => Some(*rect),
            _ => None,
        })
        .unwrap();
    let rect = view.to_view(rect);
    assert!(view.pointer_down(PointerEvent::at(rect.x + 8.0, rect.y + 8.0)));
    assert_eq!(
        view.editor().get_json()["content"][0]["content"][0]["attrs"]["checked"],
        true
    );
}

#[test]
fn ime_composition_is_one_undo_step_separate_from_later_typing() {
    let (mut view, clock) = view_with(doc(vec![p("Halo ")]), KeyPlatform::Other);
    view.select_text(6, 6, Affinity::Downstream);
    clock.advance(1_000);
    view.insert_text("d");
    assert!(view.set_composition("あ"));
    assert!(view.set_composition("あい"));
    assert_eq!(view.composition_range(), Some((7, 9)));
    assert!(view.commit_composition("愛"));
    assert_eq!(view.composition_range(), None);
    view.insert_text("!");
    assert_eq!(text_of(&view), "Halo d愛!");
    assert_eq!(head(&view), 9);
    view.run("undo", &Value::Null).unwrap();
    assert_eq!(
        text_of(&view),
        "Halo d愛",
        "typing after the composition is its own step"
    );
    view.run("undo", &Value::Null).unwrap();
    assert_eq!(text_of(&view), "Halo d", "the composition is one step");
}

#[test]
fn cancelling_a_composition_restores_the_text() {
    let (mut view, _) = view_with(doc(vec![p("ab")]), KeyPlatform::Other);
    view.select_text(2, 2, Affinity::Downstream);
    view.set_composition("x");
    assert!(view.cancel_composition());
    assert_eq!(text_of(&view), "ab");
}

#[test]
fn arrows_select_a_horizontal_rule_between_paragraphs() {
    let content = doc(vec![p("a"), json!({"type": "horizontalRule"}), p("b")]);
    let (mut view, _) = view_with(content, KeyPlatform::Other);
    view.select_text(2, 2, Affinity::Downstream);
    assert!(key(&mut view, "ArrowRight", plain()));
    assert!(view.has_node_selection());
    assert!(key(&mut view, "Backspace", plain()));
    assert_eq!(
        view.editor().get_json()["content"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn paste_text_splits_lines_and_json_round_trips() {
    let (mut view, _) = view_with(doc(vec![p("ab")]), KeyPlatform::Other);
    view.select_text(2, 2, Affinity::Downstream);
    assert!(view.paste_text("1\n2\n3"));
    assert_eq!(text_of(&view), "a1\n2\n3b");
    view.select_text(1, 5, Affinity::Downstream);
    let copied = view.copy().unwrap();
    let (text, json) = (copied.text, copied.json);
    assert_eq!(text, "a1\n");
    let end = view.editor().doc().content().size() - 1;
    view.select_text(end, end, Affinity::Downstream);
    assert!(view.paste_json(&json));
    assert!(text_of(&view).ends_with("3ba1\n"), "{}", text_of(&view));
}

#[test]
fn typing_at_the_bottom_scrolls_the_caret_into_view() {
    let blocks: Vec<Value> = (0..40).map(|i| p(&format!("baris {i}"))).collect();
    let (mut view, _) = view_with(doc(blocks), KeyPlatform::Other);
    view.set_focused(true);
    let end = view.editor().doc().content().size() - 1;
    view.select_text(end, end, Affinity::Downstream);
    assert!(view.insert_text("!"));
    assert!(view.scroll_y() > 0.0);
    let caret = view.caret_rect().unwrap();
    assert!(caret.bottom() <= view.size().1);
}

#[test]
fn caret_blinks_only_while_focused() {
    let (mut view, clock) = view_with(doc(vec![p("x")]), KeyPlatform::Other);
    assert_eq!(view.next_frame_in(), None);
    view.set_focused(true);
    assert!(view.caret_visible());
    clock.advance(BLINK_MS);
    assert!(!view.caret_visible());
    assert_eq!(view.next_frame_in(), Some(BLINK_MS));
    view.insert_text("y");
    assert!(view.caret_visible(), "typing resets the blink");
}

#[test]
fn paints_a_frame_with_selection_and_caret() {
    use skia_safe::surfaces;
    let content = doc(vec![
        json!({"type": "heading", "attrs": {"level": 1}, "content": [{"type": "text", "text": "Catatan"}]}),
        p("Pilih teks untuk memformatnya."),
        json!({"type": "bulletList", "content": [{"type": "listItem", "content": [p("satu")]}, {"type": "listItem", "content": [p("dua")]}]}),
        json!({"type": "codeBlock", "content": [{"type": "text", "text": "let x = 1;"}]}),
        p(""),
    ]);
    let (mut view, _) = view_with(content, KeyPlatform::Other);
    view.resize(560.0, 420.0, 2.0);
    view.set_focused(true);
    view.select_text(11, 18, Affinity::Downstream);
    let mut surface = surfaces::raster_n32_premul((1120, 840)).unwrap();
    view.paint(surface.canvas());
    if let Ok(dir) = std::env::var("RS_EDITOR_SNAPSHOT_DIR") {
        let image = surface.image_snapshot();
        let data = image
            .encode(None, skia_safe::EncodedImageFormat::PNG, None)
            .unwrap();
        std::fs::write(format!("{dir}/view.png"), data.as_bytes()).unwrap();
    }
    let empty = view.editor().doc().content().size() - 1;
    view.select_text(empty, empty, Affinity::Downstream);
    let mut surface = surfaces::raster_n32_premul((1120, 840)).unwrap();
    view.paint(surface.canvas());
    if let Ok(dir) = std::env::var("RS_EDITOR_SNAPSHOT_DIR") {
        let image = surface.image_snapshot();
        let data = image
            .encode(None, skia_safe::EncodedImageFormat::PNG, None)
            .unwrap();
        std::fs::write(format!("{dir}/view-placeholder.png"), data.as_bytes()).unwrap();
    }
    assert!(matches!(view.editor().selection(), Selection::Text(_)));
}

/// Sum of the pixels of `rect` (view coordinates at scale 1).
fn region_sum(view: &mut EditorView, rect: rs_editor_layout::Rect) -> u64 {
    let (width, height, _) = view.size();
    let mut surface =
        skia_safe::surfaces::raster_n32_premul((width as i32, height as i32)).unwrap();
    view.paint(surface.canvas());
    let image = surface.image_snapshot();
    let info = image.image_info();
    let mut pixels = vec![0u8; info.compute_min_byte_size()];
    let row_bytes = info.min_row_bytes();
    assert!(image.read_pixels(
        info,
        &mut pixels,
        row_bytes,
        (0, 0),
        skia_safe::image::CachingHint::Allow
    ));
    let mut sum = 0u64;
    for y in rect.y as usize..rect.bottom() as usize {
        for x in rect.x as usize..rect.right() as usize {
            let i = y * row_bytes + x * 4;
            sum += pixels[i..i + 3].iter().map(|&b| u64::from(b)).sum::<u64>();
        }
    }
    sum
}

#[test]
fn selection_shows_over_code_blocks_and_inline_backgrounds() {
    for theme in [
        rs_editor_theme::Theme::light(),
        rs_editor_theme::Theme::dark(),
    ] {
        let content = doc(vec![
            json!({"type": "codeBlock", "content": [{"type": "text", "text": "let x = 1;"}]}),
            json!({"type": "paragraph", "content": [{"type": "text", "text": "kode", "marks": [{"type": "code"}]}]}),
        ]);
        let (mut view, _) = view_with(content, KeyPlatform::Other);
        view.set_theme(theme);
        for (from, to) in [(1, 11), (13, 17)] {
            let rect = view.layout().selection_rects(from, to)[0];
            let rect = view.to_view(rect);
            view.select_text(from, from, Affinity::Downstream);
            let plain = region_sum(&mut view, rect);
            view.select_text(from, to, Affinity::Downstream);
            let selected = region_sum(&mut view, rect);
            let change = plain.abs_diff(selected) as f64 / plain.max(1) as f64;
            assert!(
                change > 0.03,
                "selection {from}..{to} invisible: {plain} vs {selected}"
            );
        }
    }
}

fn long_view(n: usize) -> EditorView {
    let blocks: Vec<Value> = (0..n)
        .map(|i| p(&format!("Paragraf {i}. {}", "isi ".repeat(i % 29 + 1))))
        .collect();
    let (view, _) = view_with(doc(blocks), KeyPlatform::Other);
    view
}

/// The view y of the text block at the vertical middle of the view.
fn middle_block(view: &mut EditorView) -> (usize, f32) {
    let (_, height, _) = view.size();
    let (_, y) = view.to_document(0.0, height / 2.0);
    let layout = view.layout();
    let text = layout
        .texts
        .iter()
        .find(|t| t.rect.bottom() >= y)
        .unwrap()
        .clone();
    (text.pos, view.to_view(text.rect).y)
}

fn view_y_of(view: &mut EditorView, pos: usize) -> f32 {
    let rect = view.layout().text_at(pos).unwrap().rect;
    view.to_view(rect).y
}

#[test]
fn scrolling_up_from_the_end_keeps_content_steady_while_measuring() {
    let mut view = long_view(3_000);
    assert!(key(
        &mut view,
        "End",
        Modifiers {
            ctrl: true,
            ..Default::default()
        }
    ));
    let bottom = view.scroll_y();
    assert!(bottom > 10_000.0);
    for _ in 0..20 {
        let (pos, before) = middle_block(&mut view);
        view.scroll_by(-250.0);
        let after = view_y_of(&mut view, pos);
        assert!(
            (after - (before + 250.0)).abs() < 0.5,
            "block {pos} moved from {before} to {after}"
        );
    }
}

#[test]
fn edits_above_the_viewport_do_not_move_it() {
    let mut view = long_view(3_000);
    view.scroll_to(30_000.0);
    let (pos, before) = middle_block(&mut view);
    // Insert a paragraph at the very start of the document.
    let inserted = {
        let state = view.state();
        let mut tr = state.tr();
        let paragraph = state.doc().child(0).clone();
        tr.insert(0, rs_editor_model::Fragment::from_node(paragraph))
            .unwrap();
        tr
    };
    let shift = view.editor().doc().child(0).node_size();
    assert!(view.dispatch(inserted));
    let after = view_y_of(&mut view, pos + shift);
    assert!(
        (after - before).abs() < 0.5,
        "moved from {before} to {after}"
    );
}

fn frames(view: &mut EditorView, clock: &ManualClock, ms: u64) {
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    for _ in 0..ms / FRAME_MS {
        clock.advance(FRAME_MS);
        view.paint(surface.canvas());
    }
}

#[test]
fn smooth_steps_ease_to_their_total_and_add_up() {
    let blocks: Vec<Value> = (0..200).map(|i| p(&format!("baris {i}"))).collect();
    let (mut view, clock) = view_with(doc(blocks), KeyPlatform::Other);
    view.scroll_smooth_by(120.0);
    assert_eq!(view.next_frame_in(), Some(FRAME_MS));
    frames(&mut view, &clock, 64);
    let partway = view.scroll_y();
    assert!(partway > 60.0 && partway < 120.0, "{partway}");
    view.scroll_smooth_by(120.0);
    frames(&mut view, &clock, 400);
    assert!(
        (view.scroll_y() - 240.0).abs() < 0.01,
        "{}",
        view.scroll_y()
    );
    assert!(!view.is_scrolling());
}

#[test]
fn a_fling_decelerates_stops_and_respects_the_edges() {
    let blocks: Vec<Value> = (0..400).map(|i| p(&format!("baris {i}"))).collect();
    let (mut view, clock) = view_with(doc(blocks), KeyPlatform::Other);
    view.fling(2_000.0);
    frames(&mut view, &clock, 160);
    let early = view.scroll_y();
    frames(&mut view, &clock, 5_000);
    let total = view.scroll_y();
    assert!(!view.is_scrolling());
    // Exponential decay covers 1 − e^(−160/τ) ≈ 27% in the first 160 ms.
    assert!((early / total - 0.274).abs() < 0.02, "{early} of {total}");
    // v·τ with τ = −1/ln(0.998) ≈ 499 ms: about 1000 px.
    assert!((900.0..1_050.0).contains(&total), "{total}");

    view.fling(-50_000.0);
    frames(&mut view, &clock, 2_000);
    assert_eq!(view.scroll_y(), 0.0);
    assert!(!view.is_scrolling(), "stops at the top edge");

    view.fling(3_000.0);
    view.pointer_down(PointerEvent::at(10.0, 10.0));
    assert!(!view.is_scrolling(), "a touch stops the momentum");
}

#[test]
fn velocity_tracker_fits_recent_samples() {
    let mut tracker = VelocityTracker::new();
    // Old, slow samples fall out of the window.
    tracker.add(0, 0.0);
    tracker.add(50, 1.0);
    for i in 0..6u64 {
        tracker.add(500 + i * 16, 100.0 + i as f32 * 8.0);
    }
    assert!(
        (tracker.velocity() - 500.0).abs() < 1.0,
        "{}",
        tracker.velocity()
    );
}

mod accessibility {
    use std::collections::HashMap;

    use accesskit::{
        Action, ActionData, ActionRequest, Node as AxNode, NodeId, Role, TextPosition,
        TextSelection, Toggled, TreeId,
    };

    use super::*;

    fn sample() -> Value {
        doc(vec![
            json!({"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "Judul"}]}),
            p(&"kata panjang ".repeat(12)),
            json!({"type": "bulletList", "content": [{"type": "listItem", "content": [p("satu")]}]}),
            json!({"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": false}, "content": [p("tugas")]}]}),
        ])
    }

    struct Tree(HashMap<NodeId, AxNode>);

    impl Tree {
        fn apply(&mut self, update: accesskit::TreeUpdate) {
            for (id, node) in update.nodes {
                self.0.insert(id, node);
            }
        }

        fn node(&self, id: NodeId) -> &AxNode {
            &self.0[&id]
        }

        /// Text runs in tree order below `id`.
        fn runs(&self, id: NodeId, out: &mut Vec<NodeId>) {
            for &child in self.node(id).children() {
                if self.node(child).role() == Role::TextRun {
                    out.push(child);
                } else {
                    self.runs(child, out);
                }
            }
        }

        fn text(&self) -> String {
            let mut runs = Vec::new();
            self.runs(EDITOR_NODE, &mut runs);
            runs.iter()
                .map(|&id| self.node(id).value().unwrap_or(""))
                .collect()
        }
    }

    #[test]
    fn the_tree_mirrors_the_document_with_lines_and_roles() {
        let (mut view, _) = view_with(sample(), KeyPlatform::Other);
        let update = view.accessibility_tree();
        assert_eq!(update.tree.as_ref().map(|t| t.root), Some(EDITOR_NODE));
        assert_eq!(update.tree_id, TreeId::ROOT);
        let mut tree = Tree(HashMap::new());
        tree.apply(update);
        assert_eq!(tree.node(EDITOR_NODE).role(), Role::MultilineTextInput);
        let content = tree.node(EDITOR_NODE).children()[0];
        let tops: Vec<&AxNode> = tree
            .node(content)
            .children()
            .iter()
            .map(|&id| tree.node(id))
            .collect();
        let roles: Vec<Role> = tops.iter().map(|n| n.role()).collect();
        assert_eq!(
            roles,
            [Role::Heading, Role::Paragraph, Role::List, Role::List]
        );
        assert_eq!(tops[0].level(), Some(2));
        assert!(
            tops[1].children().len() > 1,
            "the long paragraph wraps into several runs"
        );
        let text = tree.text();
        assert_eq!(
            text,
            format!("Judul\n{}\nsatu\ntugas\n", "kata panjang ".repeat(12))
        );
        let checkbox = tree
            .0
            .values()
            .find(|n| n.role() == Role::CheckBox)
            .unwrap();
        assert_eq!(checkbox.toggled(), Some(Toggled::False));
        assert!(checkbox.bounds().is_some(), "the checkbox has a place");
        let items: Vec<&AxNode> = tree
            .0
            .values()
            .filter(|n| n.role() == Role::ListItem)
            .collect();
        assert_eq!(items.len(), 2);
        assert!(
            items
                .iter()
                .all(|n| n.bounds().is_some_and(|b| b.height() > 0.0)),
            "list items have places"
        );
        // Character geometry is present for laid-out lines.
        let mut runs = Vec::new();
        tree.runs(EDITOR_NODE, &mut runs);
        let first = tree.node(runs[0]);
        assert_eq!(first.character_lengths().len(), "Judul\n".len());
        assert_eq!(first.character_positions().map(<[f32]>::len), Some(6));
        assert!(first.bounds().is_some());
    }

    #[test]
    fn selections_round_trip_through_text_positions() {
        let (mut view, _) = view_with(sample(), KeyPlatform::Other);
        view.select_text(3, 12, Affinity::Downstream);
        let mut tree = Tree(HashMap::new());
        tree.apply(view.accessibility_tree());
        let selection = *tree.node(EDITOR_NODE).text_selection().unwrap();
        assert_eq!(selection.anchor.character_index, 2);
        // Move the selection through the action and read it back.
        assert!(view.accessibility_action(&ActionRequest {
            action: Action::SetTextSelection,
            target_tree: TreeId::ROOT,
            target_node: EDITOR_NODE,
            data: Some(ActionData::SetTextSelection(TextSelection {
                anchor: selection.focus,
                focus: selection.anchor,
            })),
        }));
        let moved = view.editor().selection().clone();
        assert_eq!((moved.anchor(), moved.head()), (12, 3));
    }

    #[test]
    fn the_position_after_a_block_break_is_the_next_block_start() {
        let (mut view, _) = view_with(doc(vec![p("satu"), p("dua")]), KeyPlatform::Other);
        let mut tree = Tree(HashMap::new());
        tree.apply(view.accessibility_tree());
        let mut runs = Vec::new();
        tree.runs(EDITOR_NODE, &mut runs);
        // "satu\n" has five characters; index 5 is after the line break.
        let after_break = TextPosition {
            node: runs[0],
            character_index: 5,
        };
        assert!(view.accessibility_action(&ActionRequest {
            action: Action::SetTextSelection,
            target_tree: TreeId::ROOT,
            target_node: EDITOR_NODE,
            data: Some(ActionData::SetTextSelection(TextSelection {
                anchor: after_break,
                focus: after_break,
            })),
        }));
        assert_eq!(view.editor().selection().head(), 7, "the start of `dua`");
    }

    /// Moves to the next line the way AccessKit's Android adapter does for
    /// TalkBack, on a consumer-side copy of the tree.
    fn next_line(tree: &accesskit_consumer::Tree) -> Option<TextSelection> {
        let state = tree.state();
        let node = state.root();
        let current = node
            .text_selection_focus()
            .unwrap_or_else(|| node.document_range().start());
        if current.is_document_end() {
            return None;
        }
        let current = current.biased_to_start();
        let start = if current.is_line_start() {
            current
        } else {
            let start = current.forward_to_line_start();
            if start.is_document_end() {
                return None;
            }
            start
        };
        let end = start.forward_to_line_end().to_raw();
        Some(TextSelection {
            anchor: end,
            focus: end,
        })
    }

    struct Ignore;

    impl accesskit_consumer::TreeChangeHandler for Ignore {
        fn node_added(&mut self, _: &accesskit_consumer::NodeRef) {}
        fn node_updated(
            &mut self,
            _: &accesskit_consumer::NodeRef,
            _: &accesskit_consumer::NodeRef,
        ) {
        }
        fn focus_moved(
            &mut self,
            _: Option<&accesskit_consumer::NodeRef>,
            _: Option<&accesskit_consumer::NodeRef>,
        ) {
        }
        fn node_removed(&mut self, _: &accesskit_consumer::NodeRef) {}
    }

    #[test]
    fn screen_reader_line_steps_move_forward_through_every_line() {
        let content = doc(vec![
            json!({"type": "heading", "attrs": {"level": 1}, "content": [{"type": "text", "text": "Judul"}]}),
            p(&"kata panjang yang membungkus ".repeat(6)),
            json!({"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "Coba"}]}),
            json!({"type": "bulletList", "content": [
                {"type": "listItem", "content": [p("satu")]},
                {"type": "listItem", "content": [p("dua")]},
            ]}),
            p("akhir"),
        ]);
        let (mut view, _) = view_with(content, KeyPlatform::Other);
        view.set_focused(true);
        view.select_text(1, 1, Affinity::Downstream);
        let mut tree = accesskit_consumer::Tree::new(view.accessibility_tree(), true);
        let mut heads = vec![view.editor().selection().head()];
        for _ in 0..14 {
            let Some(selection) = next_line(&tree) else {
                break;
            };
            assert!(view.accessibility_action(&ActionRequest {
                action: Action::SetTextSelection,
                target_tree: TreeId::ROOT,
                target_node: EDITOR_NODE,
                data: Some(ActionData::SetTextSelection(selection)),
            }));
            heads.push(view.editor().selection().head());
            if let Some(update) = view.accessibility_update() {
                tree.update_and_process_changes(update, &mut Ignore);
            }
        }
        assert!(
            heads.windows(2).all(|w| w[1] > w[0]),
            "the caret went back: {heads:?}"
        );
        assert_eq!(
            heads.last(),
            Some(&(view.editor().doc().content().size() - 1)),
            "{heads:?}"
        );
    }

    #[test]
    fn typing_sends_only_the_edited_block() {
        let blocks: Vec<Value> = (0..200).map(|i| p(&format!("paragraf {i}"))).collect();
        let (mut view, _) = view_with(doc(blocks), KeyPlatform::Other);
        let mut tree = Tree(HashMap::new());
        let full = view.accessibility_tree();
        let total = full.nodes.len();
        tree.apply(full);
        assert!(view.accessibility_update().is_none(), "nothing changed");
        view.select_text(3, 3, Affinity::Downstream);
        view.insert_text("x");
        let update = view.accessibility_update().unwrap();
        assert!(
            update.nodes.len() <= 6,
            "{} of {total} nodes",
            update.nodes.len()
        );
        tree.apply(update);
        assert!(
            tree.text().starts_with("paxragraf 0\nparagraf 1\n"),
            "{}",
            &tree.text()[..30]
        );
        // Scrolling moves one node.
        view.scroll_by(100.0);
        let update = view.accessibility_update().unwrap();
        assert!(
            update.nodes.len() <= 3,
            "{:?}",
            update
                .nodes
                .iter()
                .map(|(id, n)| (id, n.role()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn actions_type_text_and_toggle_tasks() {
        let (mut view, _) = view_with(sample(), KeyPlatform::Other);
        let mut tree = Tree(HashMap::new());
        tree.apply(view.accessibility_tree());
        let mut runs = Vec::new();
        tree.runs(EDITOR_NODE, &mut runs);
        assert!(view.accessibility_action(&ActionRequest {
            action: Action::SetTextSelection,
            target_tree: TreeId::ROOT,
            target_node: EDITOR_NODE,
            data: Some(ActionData::SetTextSelection(TextSelection {
                anchor: TextPosition {
                    node: runs[0],
                    character_index: 5
                },
                focus: TextPosition {
                    node: runs[0],
                    character_index: 5
                },
            })),
        }));
        assert!(view.accessibility_action(&ActionRequest {
            action: Action::ReplaceSelectedText,
            target_tree: TreeId::ROOT,
            target_node: EDITOR_NODE,
            data: Some(ActionData::Value("!".into())),
        }));
        assert!(view.editor().get_text("\n").starts_with("Judul!"));
        let checkbox = *tree
            .0
            .iter()
            .find(|(_, n)| n.role() == Role::CheckBox)
            .unwrap()
            .0;
        assert!(view.accessibility_action(&ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: checkbox,
            data: None,
        }));
        assert_eq!(
            view.editor().get_json()["content"][3]["content"][0]["attrs"]["checked"],
            true
        );
    }
}

mod ime {
    use super::*;

    #[test]
    fn the_context_is_the_block_around_the_caret() {
        let (mut view, _) = view_with(doc(vec![p("satu"), p("dua tiga")]), KeyPlatform::Other);
        view.select_text(9, 12, Affinity::Downstream);
        let context = view.ime_context().unwrap();
        assert_eq!(context.start, 7);
        assert_eq!(context.text, "dua tiga");
        assert_eq!(context.selection, (2, 5));
        assert_eq!(context.text_before(10), "du");
        assert_eq!(context.text_after(2), "ig");
        assert_eq!(context.selected_text(), "a t");
    }

    #[test]
    fn caret_metrics_and_character_bounds_for_ime_popups() {
        let (mut view, _) = view_with(doc(vec![p("a👩‍💻b")]), KeyPlatform::Other);
        view.select_text(1, 1, Affinity::Downstream);
        let caret = view.ime_caret().unwrap();
        assert!(caret.top < caret.baseline && caret.baseline < caret.bottom);
        let bounds = view.ime_character_bounds(0, 7);
        assert_eq!(bounds.len(), 7);
        let rects: Vec<_> = bounds.iter().map(|b| b.unwrap()).collect();
        // "a", then five units of one emoji grapheme, then "b".
        assert!(rects[0].right() <= rects[1].x + 0.5);
        assert!(rects[1..6].iter().all(|r| *r == rects[1]));
        assert!(rects[6].x >= rects[1].right() - 0.5);
        assert!(
            (rects[0].x - caret.x).abs() < 0.5,
            "the caret sits before `a`"
        );
        assert_eq!(view.ime_character_bounds(6, 9)[2], None, "past the end");
    }

    #[test]
    fn keyboards_recompose_and_replace_words() {
        let (mut view, _) = view_with(doc(vec![p("halo dunia")]), KeyPlatform::Other);
        view.select_text(11, 11, Affinity::Downstream);
        // Gboard marks the word before the caret, then replaces it.
        assert!(view.ime_set_composing_region(5, 10));
        assert_eq!(view.composition_range(), Some((6, 11)));
        assert!(view.set_composition("dunianya"));
        assert!(view.commit_composition("dunianya"));
        assert_eq!(text_of(&view), "halo dunianya");
        view.run("undo", &Value::Null).unwrap();
        assert_eq!(
            text_of(&view),
            "halo dunia",
            "the recomposition is one undo step"
        );
    }

    #[test]
    fn a_reasserted_region_keeps_one_undo_step() {
        // The call sequence a Xiaomi keyboard sends while typing "Hal".
        let (mut view, _) = view_with(doc(vec![p("x ")]), KeyPlatform::Other);
        view.select_text(3, 3, Affinity::Downstream);
        for (text, end) in [("H", 3), ("Ha", 4), ("Hal", 5)] {
            assert!(view.set_composition(text));
            assert!(view.ime_set_composing_region(2, end));
        }
        assert!(view.commit_composition("Halo"));
        assert_eq!(text_of(&view), "x Halo");
        view.run("undo", &Value::Null).unwrap();
        assert_eq!(text_of(&view), "x ");
    }

    #[test]
    fn replacing_while_composing_tracks_the_range() {
        let (mut view, _) = view_with(doc(vec![p("ab")]), KeyPlatform::Other);
        view.select_text(3, 3, Affinity::Downstream);
        assert!(view.ime_replace(2, 2, "k", true));
        assert!(view.ime_replace(2, 3, "ka", true));
        assert!(view.ime_replace(2, 4, "か", true));
        assert_eq!(view.composition_range(), Some((3, 4)));
        view.commit_composition("か");
        assert_eq!(text_of(&view), "abか");
        assert!(view.ime_replace(0, 1, "A", false));
        assert_eq!(text_of(&view), "Abか");
    }

    #[test]
    fn delete_surrounding_never_splits_a_grapheme() {
        let (mut view, _) = view_with(doc(vec![p("x👩‍💻y")]), KeyPlatform::Other);
        view.select_text(7, 7, Affinity::Downstream);
        assert!(view.ime_delete_surrounding(1, 0));
        assert_eq!(text_of(&view), "xy");
        view.select_text(2, 2, Affinity::Downstream);
        assert!(view.ime_delete_surrounding(1, 1));
        assert_eq!(text_of(&view), "");
    }

    #[test]
    fn typed_replacements_run_input_rules() {
        let (mut view, _) = view_with(doc(vec![p("")]), KeyPlatform::Other);
        view.select_text(1, 1, Affinity::Downstream);
        assert!(view.ime_replace(0, 0, "#", false));
        assert!(view.ime_replace(1, 1, " ", false));
        assert_eq!(view.editor().get_json()["content"][0]["type"], "heading");
    }
}

#[test]
fn ime_replace_works_in_a_new_paragraph_after_enter() {
    let (mut view, _) = view_with(doc(vec![p("abc")]), KeyPlatform::Other);
    view.select_text(4, 4, Affinity::Downstream);
    assert!(key(&mut view, "Enter", plain()));
    let context = view.ime_context().unwrap();
    assert_eq!(
        (context.start, context.text.as_str(), context.selection),
        (6, "", (0, 0))
    );
    assert!(
        view.ime_replace(0, 0, "#", false),
        "{:?}",
        view.ime_context()
    );
    assert!(view.ime_replace(1, 1, "#", false));
    assert!(view.ime_replace(2, 2, " ", false));
    assert!(
        view.ime_replace(0, 0, "J", false),
        "{:?}",
        view.ime_context()
    );
    assert_eq!(text_of(&view), "abc\nJ");
}

mod touch {
    use super::*;

    /// The view point (middle of the line) of document position `pos`.
    fn point_of(view: &mut EditorView, pos: usize) -> (f32, f32) {
        let rect = view
            .layout()
            .caret_rect(pos, Affinity::Downstream, 0.0)
            .unwrap();
        let rect = view.to_view(rect);
        (rect.x, rect.center_y())
    }

    fn tap(view: &mut EditorView, (x, y): (f32, f32)) {
        view.touch(TouchPhase::Down, x, y);
        view.touch(TouchPhase::Up, x, y);
    }

    /// Paints a frame, which runs timed touch behavior.
    fn frame(view: &mut EditorView) {
        let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
        view.paint(surface.canvas());
    }

    fn menus(effects: &[Effect]) -> Vec<Option<Vec<MenuItem>>> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::SelectionMenu(menu) => Some(menu.as_ref().map(|m| m.items.clone())),
                _ => None,
            })
            .collect()
    }

    fn haptics(effects: &[Effect]) -> Vec<HapticKind> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Haptic(kind) => Some(*kind),
                _ => None,
            })
            .collect()
    }

    fn setup(content: Value) -> (EditorView, Arc<ManualClock>) {
        let (mut view, clock) = view_with(content, KeyPlatform::Other);
        view.set_focused(true);
        view.take_effects();
        (view, clock)
    }

    fn selection(view: &EditorView) -> (usize, usize) {
        let s = view.editor().selection();
        (s.from(), s.to())
    }

    #[test]
    fn a_tap_places_the_caret_with_a_handle_that_fades() {
        let (mut view, clock) = setup(doc(vec![p("halo dunia indah")]));
        let at = point_of(&mut view, 8);
        tap(&mut view, at);
        assert_eq!(selection(&view), (8, 8));
        assert!(view.take_effects().contains(&Effect::ShowKeyboard));
        let handles = view.touch_handles();
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].kind, HandleKind::Caret);
        assert!(
            handles[0].center.1 > at.1,
            "the handle hangs below the line"
        );
        clock.advance(4_100);
        frame(&mut view);
        assert!(view.touch_handles().is_empty());
    }

    #[test]
    fn tapping_the_caret_again_toggles_the_paste_menu() {
        let (mut view, clock) = setup(doc(vec![p("halo dunia indah")]));
        let at = point_of(&mut view, 8);
        tap(&mut view, at);
        clock.advance(500);
        view.take_effects();
        tap(&mut view, at);
        assert_eq!(
            menus(&view.take_effects()),
            vec![Some(vec![MenuItem::Paste, MenuItem::SelectAll])]
        );
        clock.advance(500);
        tap(&mut view, at);
        assert_eq!(menus(&view.take_effects()), vec![None]);
    }

    #[test]
    fn tapping_the_caret_handle_toggles_the_paste_menu() {
        let (mut view, clock) = setup(doc(vec![p("halo dunia indah")]));
        let at = point_of(&mut view, 8);
        tap(&mut view, at);
        let handle = view.touch_handles()[0];
        clock.advance(500);
        view.take_effects();
        tap(&mut view, handle.center);
        assert_eq!(
            menus(&view.take_effects()),
            vec![Some(vec![MenuItem::Paste, MenuItem::SelectAll])]
        );
        assert_eq!(selection(&view), (8, 8), "the caret stays");
        clock.advance(500);
        tap(&mut view, handle.center);
        assert_eq!(menus(&view.take_effects()), vec![None]);
    }

    #[test]
    fn a_long_press_selects_a_word_and_dragging_extends_by_words() {
        let (mut view, clock) = setup(doc(vec![p("halo dunia indah")]));
        let (x, y) = point_of(&mut view, 8);
        view.touch(TouchPhase::Down, x, y);
        assert_eq!(
            view.next_frame_in(),
            Some(400),
            "the long press wakes the view"
        );
        clock.advance(450);
        frame(&mut view);
        assert_eq!(selection(&view), (6, 11));
        let effects = view.take_effects();
        assert_eq!(haptics(&effects), vec![HapticKind::LongPress]);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::Magnifier(Some(_))))
        );
        let (x2, _) = point_of(&mut view, 14);
        view.touch(TouchPhase::Move, x2, y);
        assert_eq!(selection(&view), (6, 17), "whole words");
        assert_eq!(
            haptics(&view.take_effects()),
            vec![HapticKind::TextHandleMove]
        );
        view.touch(TouchPhase::Up, x2, y);
        let effects = view.take_effects();
        assert_eq!(
            menus(&effects),
            vec![Some(vec![
                MenuItem::Cut,
                MenuItem::Copy,
                MenuItem::Paste,
                MenuItem::SelectAll
            ])]
        );
        assert!(effects.contains(&Effect::Magnifier(None)));
    }

    #[test]
    fn handles_grow_by_words_shrink_by_characters_and_never_cross() {
        let (mut view, clock) = setup(doc(vec![p("halo dunia indah sekali")]));
        let at = point_of(&mut view, 8);
        tap(&mut view, at);
        clock.advance(100);
        tap(&mut view, at);
        assert_eq!(selection(&view), (6, 11), "double tap selects the word");
        let end = view
            .touch_handles()
            .into_iter()
            .find(|h| h.kind == HandleKind::End)
            .unwrap();
        let (x11, _) = point_of(&mut view, 11);
        let grab = end.center;
        let drag_to = |view: &mut EditorView, pos: usize| {
            let (x, _) = point_of(view, pos);
            view.touch(TouchPhase::Move, grab.0 + (x - x11), grab.1);
        };
        view.touch(TouchPhase::Down, grab.0, grab.1);
        drag_to(&mut view, 14);
        assert_eq!(selection(&view), (6, 17), "growing snaps to the word end");
        drag_to(&mut view, 15);
        assert_eq!(
            selection(&view),
            (6, 17),
            "moving on inside the word keeps its end"
        );
        drag_to(&mut view, 13);
        assert_eq!(selection(&view), (6, 13), "moving back goes by character");
        drag_to(&mut view, 19);
        assert_eq!(
            selection(&view),
            (6, 24),
            "moving outward again snaps to words"
        );
        drag_to(&mut view, 3);
        assert_eq!(selection(&view), (6, 24), "the end never passes the start");
        view.touch(TouchPhase::Up, grab.0, grab.1);
        assert!(menus(&view.take_effects()).last().unwrap().is_some());
    }

    #[test]
    fn scrolling_hides_the_menu_and_brings_it_back() {
        let blocks: Vec<Value> = (0..60).map(|i| p(&format!("baris nomor {i}"))).collect();
        let (mut view, clock) = setup(doc(blocks));
        let at = point_of(&mut view, 3);
        tap(&mut view, at);
        clock.advance(100);
        tap(&mut view, at);
        assert!(menus(&view.take_effects()).last().unwrap().is_some());
        view.touch(TouchPhase::Down, 200.0, 300.0);
        clock.advance(16);
        view.touch(TouchPhase::Move, 200.0, 250.0);
        clock.advance(16);
        view.touch(TouchPhase::Move, 200.0, 200.0);
        view.touch(TouchPhase::Up, 200.0, 200.0);
        assert_eq!(menus(&view.take_effects()), vec![None]);
        assert!(view.is_scrolling());
        for _ in 0..400 {
            clock.advance(FRAME_MS);
            frame(&mut view);
        }
        assert!(!view.is_scrolling());
        assert!(
            menus(&view.take_effects()).last().unwrap().is_some(),
            "back after the fling"
        );
    }

    #[test]
    fn typing_ends_touch_selection() {
        let (mut view, clock) = setup(doc(vec![p("halo dunia")]));
        let at = point_of(&mut view, 8);
        tap(&mut view, at);
        clock.advance(100);
        tap(&mut view, at);
        view.take_effects();
        view.insert_text("x");
        assert!(view.touch_handles().is_empty());
        assert_eq!(menus(&view.take_effects()), vec![None]);
    }

    #[test]
    fn menu_commands_copy_cut_paste_and_select_all() {
        let (mut view, clock) = setup(doc(vec![p("halo dunia indah")]));
        let at = point_of(&mut view, 8);
        tap(&mut view, at);
        clock.advance(100);
        tap(&mut view, at);
        assert_eq!(
            view.menu_action(MenuItem::Copy, None, None)
                .map(|c| c.text)
                .as_deref(),
            Some("dunia")
        );
        assert_eq!(selection(&view), (11, 11), "copy collapses the selection");
        assert!(view.touch_handles().is_empty());
        assert_eq!(view.menu_action(MenuItem::Paste, Some("!"), None), None);
        assert_eq!(text_of(&view), "halo dunia! indah");
        view.take_effects();
        view.menu_action(MenuItem::SelectAll, None, None);
        assert_eq!(
            menus(&view.take_effects()).last().unwrap().clone(),
            Some(vec![MenuItem::Cut, MenuItem::Copy, MenuItem::Paste])
        );
        assert_eq!(view.touch_handles().len(), 2);
        assert_eq!(
            view.menu_action(MenuItem::Cut, None, None)
                .map(|c| c.text)
                .as_deref(),
            Some("halo dunia! indah")
        );
        assert_eq!(text_of(&view), "");
    }

    #[test]
    fn dragging_near_the_bottom_edge_scrolls_and_extends() {
        let blocks: Vec<Value> = (0..80).map(|i| p(&format!("baris nomor {i}"))).collect();
        let (mut view, clock) = setup(doc(blocks));
        let at = point_of(&mut view, 3);
        view.touch(TouchPhase::Down, at.0, at.1);
        clock.advance(450);
        frame(&mut view);
        let (_, height, _) = view.size();
        view.touch(TouchPhase::Move, at.0, height - 4.0);
        let before = selection(&view).1;
        assert_eq!(view.next_frame_in(), Some(FRAME_MS));
        for _ in 0..60 {
            clock.advance(FRAME_MS);
            frame(&mut view);
        }
        assert!(view.scroll_y() > 100.0, "{}", view.scroll_y());
        assert!(selection(&view).1 > before + 30);
    }

    #[test]
    fn the_caret_stays_visible_when_the_keyboard_shrinks_the_view() {
        let blocks: Vec<Value> = (0..60).map(|i| p(&format!("baris nomor {i}"))).collect();
        let (mut view, _) = setup(doc(blocks));
        let (_, height, scale) = view.size();
        // Tap a line near the bottom of the view.
        let target = view.to_document(100.0, height - 30.0);
        let pos = match view.layout().hit(target.0, target.1) {
            Some(rs_editor_layout::Hit::Text { pos, .. }) => pos,
            _ => panic!("no text near the bottom"),
        };
        let at = point_of(&mut view, pos);
        tap(&mut view, at);
        let before = view.caret_rect().unwrap();
        assert!(before.bottom() <= height);
        // The keyboard takes 60% of the screen.
        view.resize(640.0, height * 0.4, scale);
        let after = view.caret_rect().unwrap();
        assert!(
            after.y >= 0.0 && after.bottom() <= height * 0.4,
            "caret at {after:?}"
        );
        assert!(view.scroll_y() > 0.0);
    }

    #[test]
    fn a_caret_scrolled_away_is_left_alone_on_resize() {
        let blocks: Vec<Value> = (0..60).map(|i| p(&format!("baris nomor {i}"))).collect();
        let (mut view, _) = setup(doc(blocks));
        view.select_text(3, 3, Affinity::Downstream);
        view.scroll_to(800.0);
        let (_, height, scale) = view.size();
        view.resize(640.0, height * 0.4, scale);
        assert_eq!(view.scroll_y(), 800.0);
    }

    #[test]
    fn on_ios_a_long_press_moves_the_caret_under_a_magnifier() {
        let (mut view, clock) = setup(doc(vec![p("halo dunia indah")]));
        view.set_interaction_profile(InteractionProfile::ios());
        let (x, y) = point_of(&mut view, 3);
        view.touch(TouchPhase::Down, x, y);
        clock.advance(550);
        frame(&mut view);
        assert_eq!(selection(&view), (3, 3), "no word selection");
        let effects = view.take_effects();
        assert_eq!(haptics(&effects), vec![HapticKind::LongPress]);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::Magnifier(Some(_))))
        );
        let (x2, _) = point_of(&mut view, 12);
        view.touch(TouchPhase::Move, x2, y);
        assert_eq!(selection(&view), (12, 12));
        view.touch(TouchPhase::Up, x2, y);
        let effects = view.take_effects();
        assert!(effects.contains(&Effect::Magnifier(None)));
        assert!(menus(&effects).is_empty(), "moving the caret shows no menu");
    }

    #[test]
    fn apple_handles_put_knobs_above_and_below_the_selection() {
        let (mut view, clock) = setup(doc(vec![p("halo dunia indah")]));
        view.set_interaction_profile(InteractionProfile::ios());
        let at = point_of(&mut view, 8);
        tap(&mut view, at);
        clock.advance(100);
        tap(&mut view, at);
        assert_eq!(selection(&view), (6, 11));
        let handles = view.touch_handles();
        let start = handles
            .iter()
            .find(|h| h.kind == HandleKind::Start)
            .unwrap();
        let end = handles.iter().find(|h| h.kind == HandleKind::End).unwrap();
        assert!(start.center.1 < start.line.y, "start knob above the line");
        assert!(end.center.1 > end.line.bottom(), "end knob below the line");
        assert_eq!(start.center.0, start.line.x);
    }

    #[test]
    fn a_menu_closed_by_the_system_can_be_opened_again() {
        let (mut view, clock) = setup(doc(vec![p("halo dunia indah")]));
        let at = point_of(&mut view, 8);
        tap(&mut view, at);
        clock.advance(500);
        tap(&mut view, at);
        assert!(menus(&view.take_effects()).last().unwrap().is_some());
        view.selection_menu_dismissed();
        clock.advance(500);
        tap(&mut view, at);
        assert!(
            menus(&view.take_effects()).last().unwrap().is_some(),
            "the next tap on the caret shows it again"
        );
    }

    #[test]
    fn handles_are_painted_in_the_selection_color() {
        let (mut view, clock) = setup(doc(vec![p("halo dunia indah")]));
        let at = point_of(&mut view, 8);
        tap(&mut view, at);
        clock.advance(100);
        tap(&mut view, at);
        let end = view
            .touch_handles()
            .into_iter()
            .find(|h| h.kind == HandleKind::End)
            .unwrap();
        let (width, height, _) = view.size();
        let mut surface =
            skia_safe::surfaces::raster_n32_premul((width as i32, height as i32)).unwrap();
        view.paint(surface.canvas());
        let image = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (1, 1),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut pixel = [0u8; 4];
        assert!(image.read_pixels(
            &info,
            &mut pixel,
            4,
            (end.center.0 as i32, end.center.1 as i32),
            skia_safe::image::CachingHint::Allow,
        ));
        let expected = view.theme().palette.selection.with_alpha(1.0);
        let argb = expected.argb();
        let rgb = [(argb >> 16) as u8, (argb >> 8) as u8, argb as u8];
        let close = pixel[..3].iter().zip(rgb).all(|(a, b)| a.abs_diff(b) <= 2);
        assert!(close, "handle pixel {:?} vs {:?}", &pixel[..3], rgb);
    }
}

#[test]
fn html_from_copy_pastes_back_exactly_and_other_html_merges_in() {
    let content = doc(vec![
        json!({"type": "paragraph", "content": [{"type": "text", "text": "tebal", "marks": [{"type": "bold"}]}, {"type": "text", "text": " biasa"}]}),
        json!({"type": "bulletList", "content": [{"type": "listItem", "content": [p("item")]}]}),
    ]);
    let (mut view, _) = view_with(content, KeyPlatform::Other);
    let end = view.editor().doc().content().size();
    view.select_text(1, end - 1, Affinity::Downstream);
    let copied = view.copy().unwrap();
    assert!(
        copied.html.contains("<strong>tebal</strong>"),
        "{}",
        copied.html
    );
    view.select_text(end - 1, end - 1, Affinity::Downstream);
    let before = view.editor().get_json();
    view.select_text(1, 1, Affinity::Downstream);
    assert!(view.paste_html(&copied.html));
    let after = view.editor().get_json();
    assert_eq!(
        after["content"][0]["content"][0]["marks"][0]["type"],
        "bold"
    );
    assert!(
        after["content"].as_array().unwrap().len() > before["content"].as_array().unwrap().len()
    );

    // Inline HTML from another app joins the paragraph at the caret.
    let (mut view, _) = view_with(doc(vec![p("ab")]), KeyPlatform::Other);
    view.select_text(2, 2, Affinity::Downstream);
    assert!(view.paste_html(r#"<b style="font-weight:normal" id="docs-internal-guid-1"><span style="font-weight:700">X</span></b>"#));
    assert_eq!(text_of(&view), "aXb");
    assert_eq!(
        view.editor().get_json()["content"][0]["content"][1]["marks"][0]["type"],
        "bold"
    );
}
