use std::rc::Rc;

use rs_editor_model::testing::*;
use rs_editor_model::{Node, Schema, v};
use rs_editor_theme::Theme;
use serde_json::json;

use crate::*;

fn engine() -> LayoutEngine {
    LayoutEngine::new(Theme::light(), false)
}

/// The StarterKit-like schema with Tiptap names, built from JSON.
fn tiptap(json: serde_json::Value) -> Node {
    use rs_editor_model::{AttrSpec, MarkSpec, NodeSpec, SchemaSpec};
    let schema = Schema::new(
        SchemaSpec::default()
            .node("doc", NodeSpec::new().content("block+"))
            .node(
                "paragraph",
                NodeSpec::new()
                    .content("inline*")
                    .group("block")
                    .attr("textAlign", AttrSpec::with_default("left")),
            )
            .node("text", NodeSpec::new().group("inline"))
            .node(
                "heading",
                NodeSpec::new()
                    .content("inline*")
                    .group("block")
                    .attr("level", AttrSpec::with_default(1)),
            )
            .node(
                "blockquote",
                NodeSpec::new().content("block+").group("block"),
            )
            .node(
                "codeBlock",
                NodeSpec::new()
                    .content("text*")
                    .marks("")
                    .group("block")
                    .code(),
            )
            .node("horizontalRule", NodeSpec::new().group("block"))
            .node(
                "hardBreak",
                NodeSpec::new().inline().group("inline").leaf_text("\n"),
            )
            .node(
                "bulletList",
                NodeSpec::new().content("listItem+").group("block list"),
            )
            .node(
                "orderedList",
                NodeSpec::new()
                    .content("listItem+")
                    .group("block list")
                    .attr("start", AttrSpec::with_default(1)),
            )
            .node("listItem", NodeSpec::new().content("paragraph block*"))
            .node(
                "taskList",
                NodeSpec::new().content("taskItem+").group("block list"),
            )
            .node(
                "taskItem",
                NodeSpec::new()
                    .content("paragraph block*")
                    .attr("checked", AttrSpec::with_default(false)),
            )
            .mark("bold", MarkSpec::new())
            .mark("code", MarkSpec::new().code()),
    )
    .unwrap();
    schema.node_from_json(&json).unwrap()
}

fn para(text: &str) -> serde_json::Value {
    json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
}

#[test]
fn every_grapheme_boundary_round_trips_through_caret_and_hit() {
    for sample in [
        "Halo dunia, apa kabar?",
        "العربية والعالم",
        "हिन्दी भाषा",
        "mix عربي text",
    ] {
        let doc = tiptap(json!({"type": "doc", "content": [para(sample)]}));
        let layout = engine().layout(&doc, 600.0);
        let text = &layout.texts[0];
        assert_eq!(text.pos, 1);
        let mut last_hit = None;
        for &offset in text.layout.index().boundaries() {
            let caret = text.layout.caret(offset, Affinity::Downstream);
            assert!(caret.height > 8.0, "{sample}: caret at {offset}");
            let (hit, _) = text.layout.offset_at(caret.x, caret.y + caret.height / 2.0);
            // At a bidi boundary two offsets share an x; the hit is one of them.
            let same_x = text.layout.caret(hit, Affinity::Downstream).x;
            assert!(
                (same_x - caret.x).abs() < 1.0,
                "{sample}: offset {offset} hit {hit}"
            );
            last_hit = Some(hit);
        }
        assert!(last_hit.is_some());
    }
}

#[test]
fn ltr_carets_advance_left_to_right() {
    let doc = tiptap(json!({"type": "doc", "content": [para("Halo dunia")]}));
    let layout = engine().layout(&doc, 600.0);
    let text = &layout.texts[0].layout;
    let xs: Vec<f32> = (0..=10)
        .map(|o| text.caret(o, Affinity::Downstream).x)
        .collect();
    assert_eq!(xs[0], 0.0);
    assert!(xs.windows(2).all(|w| w[1] > w[0]), "{xs:?}");
}

#[test]
fn soft_wraps_use_affinity() {
    let doc = tiptap(
        json!({"type": "doc", "content": [para("one two three four five six seven eight nine ten")]}),
    );
    let layout = engine().layout(&doc, 120.0);
    let text = &layout.texts[0].layout;
    assert!(text.lines().len() >= 3, "wraps at 120px");
    let second = text.lines()[1];
    let up = text.caret(second.start, Affinity::Upstream);
    let down = text.caret(second.start, Affinity::Downstream);
    assert!(up.y < down.y, "upstream stays at the end of the first line");
    assert!(down.x < 1.0, "downstream starts the next line");
    assert_eq!(text.line_index(second.start, Affinity::Upstream), 0);
    assert_eq!(text.line_index(second.start, Affinity::Downstream), 1);
}

#[test]
fn hard_breaks_and_empty_blocks_have_carets() {
    let doc = tiptap(json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": "a"}, {"type": "hardBreak"}, {"type": "text", "text": "b"}]},
        {"type": "paragraph"},
    ]}));
    let layout = engine().layout(&doc, 400.0);
    let first = &layout.texts[0].layout;
    assert_eq!(first.len(), 3);
    assert_eq!(first.lines().len(), 2);
    let before_break = first.caret(1, Affinity::Downstream);
    let after_break = first.caret(2, Affinity::Downstream);
    assert!(after_break.y > before_break.y);
    assert!(after_break.x < 1.0);
    let empty = &layout.texts[1];
    assert_eq!(empty.pos, 6);
    let caret = layout.caret_rect(6, Affinity::Downstream, 1.5).unwrap();
    assert!(caret.height > 10.0 && caret.y >= empty.rect.y);
}

#[test]
fn blocks_follow_document_positions_and_theme_sizes() {
    let doc = tiptap(json!({"type": "doc", "content": [
        {"type": "heading", "attrs": {"level": 1}, "content": [{"type": "text", "text": "Judul"}]},
        para("Paragraf"),
        {"type": "bulletList", "content": [
            {"type": "listItem", "content": [para("satu")]},
            {"type": "listItem", "content": [para("dua")]}
        ]},
        {"type": "orderedList", "attrs": {"start": 3}, "content": [{"type": "listItem", "content": [para("tiga")]}]},
        {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": true}, "content": [para("tugas")]}]},
        {"type": "blockquote", "content": [para("kutipan")]},
        {"type": "codeBlock", "content": [{"type": "text", "text": "let x = 1;"}]},
        {"type": "horizontalRule"},
    ]}));
    let layout = engine().layout(&doc, 500.0);
    let positions: Vec<usize> = layout.texts.iter().map(|t| t.pos).collect();
    let mut expected = Vec::new();
    doc.descendants(&mut |node, pos, _, _| {
        if node.is_textblock() {
            expected.push(pos + 1);
        }
        true
    });
    assert_eq!(positions, expected);
    let heading = &layout.texts[0];
    let paragraph = &layout.texts[1];
    assert!(heading.rect.height >= 40.0 && paragraph.rect.height >= 28.0);
    assert!(paragraph.rect.y > heading.rect.bottom());
    let list_text = &layout.texts[2];
    assert_eq!(list_text.rect.x, Theme::light().spacing.list_indent);
    assert!(
        layout
            .decorations
            .iter()
            .any(|d| matches!(d, Decoration::Bullet { .. }))
    );
    assert!(
        layout
            .decorations
            .iter()
            .any(|d| matches!(d, Decoration::Number { .. }))
    );
    assert!(
        layout
            .decorations
            .iter()
            .any(|d| matches!(d, Decoration::Checkbox { checked: true, .. }))
    );
    assert!(
        layout
            .decorations
            .iter()
            .any(|d| matches!(d, Decoration::QuoteBar { .. }))
    );
    assert!(
        layout
            .decorations
            .iter()
            .any(|d| matches!(d, Decoration::CodeBackground { .. }))
    );
    assert!(
        layout
            .decorations
            .iter()
            .any(|d| matches!(d, Decoration::Rule { .. }))
    );
    let rule = layout
        .blocks
        .iter()
        .find(|b| b.node.type_name() == "horizontalRule")
        .unwrap();
    assert_eq!(
        layout.hit(rule.rect.x + 10.0, rule.rect.center_y()),
        Some(Hit::Node { pos: rule.pos })
    );
    assert!(layout.height > layout.texts.last().unwrap().rect.bottom());
}

#[test]
fn unchanged_blocks_reuse_their_paragraphs() {
    let mut engine = engine();
    let doc = tiptap(json!({"type": "doc", "content": [para("satu"), para("dua")]}));
    let first = engine.layout(&doc, 400.0);
    // Edit only the second paragraph; the first keeps its node identity.
    let edited = doc
        .replace(
            7,
            7,
            &rs_editor_model::Slice::new(
                rs_editor_model::Fragment::from_node(
                    doc.node_type()
                        .schema()
                        .text("!", Default::default())
                        .unwrap(),
                ),
                0,
                0,
            ),
        )
        .unwrap();
    let second = engine.layout(&edited, 400.0);
    assert!(Rc::ptr_eq(&first.texts[0].layout, &second.texts[0].layout));
    assert!(!Rc::ptr_eq(&first.texts[1].layout, &second.texts[1].layout));
    assert_eq!(engine.cached_blocks(), 2);
}

#[test]
fn vertical_movement_keeps_the_goal_column() {
    let doc = tiptap(json!({"type": "doc", "content": [para("abcdefgh"), para("ABCDEFGHIJ")]}));
    let layout = engine().layout(&doc, 400.0);
    let start = 5;
    let caret = layout.caret_rect(start, Affinity::Downstream, 0.0).unwrap();
    let (down, _) = layout
        .move_vertical(start, Affinity::Downstream, false, caret.x)
        .unwrap();
    assert!(down > layout.texts[1].pos && down < layout.texts[1].end());
    let (up, _) = layout
        .move_vertical(down, Affinity::Downstream, true, caret.x)
        .unwrap();
    assert_eq!(up, start);
    assert_eq!(
        layout.move_vertical(start, Affinity::Downstream, true, caret.x),
        None
    );
}

#[test]
fn visual_movement_follows_the_screen_in_rtl_text() {
    let doc = tiptap(json!({"type": "doc", "content": [para("سلام")]}));
    let layout = engine().layout(&doc, 400.0);
    let text = &layout.texts[0];
    // In right-to-left text, logical start is the right edge.
    let start = layout
        .caret_rect(text.pos, Affinity::Downstream, 0.0)
        .unwrap();
    let end = layout
        .caret_rect(text.end(), Affinity::Downstream, 0.0)
        .unwrap();
    assert!(start.x > end.x);
    let (left, _) = layout
        .move_visual(text.pos, Affinity::Downstream, VisualDirection::Left)
        .unwrap();
    assert!(left > text.pos, "moving left in Arabic advances logically");
}

#[test]
fn selection_rects_cover_text_across_blocks() {
    let doc = tiptap(json!({"type": "doc", "content": [para("satu"), para("dua")]}));
    let layout = engine().layout(&doc, 400.0);
    let rects = layout.selection_rects(2, 9);
    assert!(rects.len() >= 3, "text in both blocks plus the line break");
    assert!(rects.iter().all(|r| r.height >= 20.0));
    assert!(layout.selection_rects(3, 3).is_empty());
}

#[test]
fn renders_a_document_frame() {
    use skia_safe::{Color, surfaces};
    let doc = tiptap(json!({"type": "doc", "content": [
        {"type": "heading", "attrs": {"level": 1}, "content": [{"type": "text", "text": "Rs Editor"}]},
        para("Editor rich text dengan Skia."),
        {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": true}, "content": [para("Tugas")]}]},
    ]}));
    let theme = Theme::light();
    let layout = LayoutEngine::new(theme.clone(), false).layout(&doc, 480.0);
    let mut surface = surfaces::raster_n32_premul((480, layout.height.ceil() as i32 + 8)).unwrap();
    surface.canvas().clear(Color::WHITE);
    render::paint_document(
        surface.canvas(),
        &layout,
        &theme,
        Rect::new(0.0, 0.0, 480.0, layout.height),
    );
    let image = surface.image_snapshot();
    let data = image
        .encode(None, skia_safe::EncodedImageFormat::PNG, None)
        .unwrap();
    if let Ok(dir) = std::env::var("RS_EDITOR_SNAPSHOT_DIR") {
        std::fs::write(format!("{dir}/layout.png"), data.as_bytes()).unwrap();
    }
    let info = image.image_info();
    let mut pixels = vec![0u8; info.min_row_bytes() * info.height() as usize];
    assert!(image.read_pixels(
        info,
        &mut pixels,
        info.min_row_bytes(),
        (0, 0),
        skia_safe::image::CachingHint::Allow
    ));
    let dark = pixels
        .chunks(4)
        .filter(|p| p[0] < 100 && p[1] < 100 && p[2] < 100)
        .count();
    assert!(dark > 500, "text was drawn ({dark} dark pixels)");
}

#[test]
fn builder_documents_from_the_test_schema_lay_out() {
    let doc = doc(v![
        p(v!["hello ", em(v!["world"])]),
        blockquote(v![p(v!["quote"])])
    ])
    .node;
    let layout = engine().layout(&doc, 300.0);
    assert_eq!(layout.texts.len(), 2);
}

fn long_doc(n: usize) -> Node {
    let blocks: Vec<serde_json::Value> = (0..n)
        .map(|i| {
            if i % 10 == 0 {
                json!({"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": format!("Bagian {i}")}]})
            } else {
                para(&"kata ".repeat(i % 37 + 1))
            }
        })
        .collect();
    tiptap(json!({"type": "doc", "content": blocks}))
}

fn places(layout: &DocumentLayout) -> Vec<(usize, usize, f32, f32)> {
    layout
        .tops
        .iter()
        .map(|t| (t.pos, t.size, t.y, t.height))
        .collect()
}

fn check_tops(layout: &DocumentLayout) {
    let tops: Vec<TopBlock> = layout.tops.iter().collect();
    assert_eq!(tops.len(), layout.tops.len());
    for (index, top) in tops.iter().enumerate() {
        assert_eq!(layout.tops.get(index), Some(*top));
        assert_eq!(layout.top_at_pos(top.pos), Some(index));
        assert_eq!(layout.top_at_pos(top.end() - 1), Some(index));
        if top.height > 0.0 {
            assert_eq!(layout.top_at_y(top.y + top.height / 2.0), Some(index));
        }
    }
}

#[test]
fn incremental_layout_matches_a_fresh_one_across_edits() {
    let mut doc = long_doc(300);
    let mut incremental = engine();
    incremental.layout(&doc, 420.0);
    // Edits around chunk boundaries (64 blocks), at both ends, and ones
    // that change the number of blocks.
    let targets = [0usize, 63, 64, 65, 127, 128, 150, 299];
    for (step, &target) in targets.iter().enumerate() {
        let layout = incremental.layout(&doc, 420.0);
        let index = target.min(layout.tops.len() - 1);
        let top = layout.tops.get(index).unwrap();
        doc = match step % 3 {
            0 => {
                let text = doc
                    .node_type()
                    .schema()
                    .text("x", Default::default())
                    .unwrap();
                doc.replace(
                    top.pos + 1,
                    top.pos + 1,
                    &rs_editor_model::Slice::new(rs_editor_model::Fragment::from_node(text), 0, 0),
                )
                .unwrap()
            }
            1 => {
                let copy = doc.child(1).clone();
                doc.replace(
                    top.pos,
                    top.pos,
                    &rs_editor_model::Slice::new(rs_editor_model::Fragment::from_node(copy), 0, 0),
                )
                .unwrap()
            }
            _ => doc
                .replace(top.pos, top.end(), &rs_editor_model::Slice::empty())
                .unwrap(),
        };
        let updated = incremental.layout(&doc, 420.0);
        let fresh = engine().layout(&doc, 420.0);
        assert_eq!(
            places(&updated),
            places(&fresh),
            "after edit {step} at block {index}"
        );
        assert_eq!(updated.height, fresh.height);
        check_tops(&updated);
    }
}

#[test]
fn windowed_layout_measures_only_what_is_near() {
    let doc = long_doc(2_000);
    let mut engine = engine();
    let layout = engine.layout_window(&doc, 420.0, Some((0.0, 800.0)), &[]);
    let measured = engine.cached_blocks();
    assert!(measured > 5 && measured < 60, "measured {measured}");
    assert!(layout.covers(0.0, 800.0));
    assert!(layout.texts.iter().all(|t| t.rect.y <= 800.0));
    assert!(
        layout.height > 2_000.0 * 20.0,
        "estimated height {}",
        layout.height
    );
    check_tops(&layout);

    // Blocks in the window sit where a full layout puts them.
    let full = super::LayoutEngine::new(Theme::light(), false).layout(&doc, 420.0);
    for text in &layout.texts {
        let same = full.text_at(text.pos).unwrap();
        assert_eq!(same.rect, text.rect);
    }

    // Asking for a far position lays out its block and neighbours.
    let far = full.tops.get(1_500).unwrap().pos + 1;
    let layout = engine.layout_window(&doc, 420.0, Some((0.0, 800.0)), &[far]);
    assert!(layout.shows(far));
    assert!(layout.caret_rect(far, Affinity::Downstream, 1.0).is_some());
    assert!(layout.text_at(far + 1).is_some());
}
