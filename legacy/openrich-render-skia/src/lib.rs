//! First native text-layout and drawing backend using Skia Paragraph.
//!
//! The host owns the surface and passes its canvas to [`SkiaParagraph::paint`].

use openrich_document::{Affinity as EditorAffinity, Block, Mark};
use openrich_layout::{
    LayoutBackend, ParagraphLayout, Point as LayoutPoint, Rect as LayoutRect, Size as LayoutSize,
    TextHit, TextIndexError, TextIndexMap,
};
use openrich_render::{DocumentLayout, Renderer};
use skia_safe::{
    Canvas, Color, Data, FontMgr, FontStyle, Paint, Point, Rect,
    textlayout::{
        Affinity as SkiaAffinity, FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle,
        PositionWithAffinity, RectHeightStyle, RectWidthStyle, TextStyle, TypefaceFontProvider,
    },
};
use thiserror::Error;

/// Shared, redistributable font bytes used by Skia and browser input adapters.
pub const BUNDLED_FONTS: &[(&str, &[u8])] = &[
    (
        "LiberationSans-Regular.ttf",
        include_bytes!("../assets/fonts/LiberationSans-Regular.ttf"),
    ),
    (
        "LiberationSans-Bold.ttf",
        include_bytes!("../assets/fonts/LiberationSans-Bold.ttf"),
    ),
    (
        "LiberationSans-Italic.ttf",
        include_bytes!("../assets/fonts/LiberationSans-Italic.ttf"),
    ),
    (
        "LiberationSans-BoldItalic.ttf",
        include_bytes!("../assets/fonts/LiberationSans-BoldItalic.ttf"),
    ),
    (
        "LiberationMono-Regular.ttf",
        include_bytes!("../assets/fonts/LiberationMono-Regular.ttf"),
    ),
    (
        "LiberationMono-Bold.ttf",
        include_bytes!("../assets/fonts/LiberationMono-Bold.ttf"),
    ),
    (
        "LiberationMono-Italic.ttf",
        include_bytes!("../assets/fonts/LiberationMono-Italic.ttf"),
    ),
    (
        "LiberationMono-BoldItalic.ttf",
        include_bytes!("../assets/fonts/LiberationMono-BoldItalic.ttf"),
    ),    (
        "NotoSansArabic-Regular.ttf",
        include_bytes!("../assets/fonts/NotoSansArabic-Regular.ttf"),
    ),
    (
        "NotoSansArabic-Bold.ttf",
        include_bytes!("../assets/fonts/NotoSansArabic-Bold.ttf"),
    ),
    (
        "NotoSansDevanagari-Regular.ttf",
        include_bytes!("../assets/fonts/NotoSansDevanagari-Regular.ttf"),
    ),
    (
        "NotoSansDevanagari-Bold.ttf",
        include_bytes!("../assets/fonts/NotoSansDevanagari-Bold.ttf"),
    ),
];

/// A laid out Skia paragraph with a map back to editor grapheme offsets.
pub struct SkiaParagraph {
    paragraph: Paragraph,
    index_map: TextIndexMap,
}

impl SkiaParagraph {
    /// Shapes and lays out plain text at the supplied logical width.
    pub fn new(text: &str, width: f32, font_size: f32) -> Result<Self, SkiaLayoutError> {
        let (paragraph_style, fonts) = paragraph_config(width, font_size)?;
        let mut builder = ParagraphBuilder::new(&paragraph_style, fonts);
        builder.add_text(text);
        let mut paragraph = builder.build();
        paragraph.layout(width);

        Ok(Self {
            paragraph,
            index_map: TextIndexMap::from_text(text),
        })
    }

    /// Builds a paragraph from text-bearing document blocks.
    ///
    /// Applies the marks represented by the document schema to each text run.
    pub fn from_block(block: &Block, width: f32, font_size: f32) -> Result<Self, SkiaLayoutError> {
        let content = block
            .inline_content()
            .ok_or(SkiaLayoutError::UnsupportedBlock)?;
        let heading_level = match block {
            Block::Heading { level, .. } => Some(*level),
            Block::Formatted { format, .. } => format.heading_level,
            _ => None,
        };
        let effective_size = match heading_level {
            Some(1) => font_size * 1.65,
            Some(2) => font_size * 1.4,
            Some(3) => font_size * 1.2,
            Some(_) => font_size * 1.08,
            None => font_size,
        };
        let (mut paragraph_style, fonts) = paragraph_config(width, effective_size)?;
        if let Block::Formatted { format, .. } = block {
            use openrich_document::TextAlignment;
            use skia_safe::textlayout::TextAlign;
            paragraph_style.set_text_align(match format.alignment {
                TextAlignment::Left => TextAlign::Left,
                TextAlignment::Center => TextAlign::Center,
                TextAlignment::Right => TextAlign::Right,
                TextAlignment::Justify => TextAlign::Justify,
            });
        }
        let mut builder = ParagraphBuilder::new(&paragraph_style, fonts);
        let mut text = String::new();
        for inline in content {
            match inline {
                openrich_document::Inline::Text { text: run, marks } => {
                    text.push_str(run);
                    let mut style = text_style(effective_size, marks);
                    if heading_level.is_some() {
                        let italic = marks.iter().any(|mark| matches!(mark, Mark::Italic));
                        style.set_font_style(if italic {
                            FontStyle::bold_italic()
                        } else {
                            FontStyle::bold()
                        });
                    }
                    builder.push_style(&style);
                    builder.add_text(run);
                    builder.pop();
                }
            }
        }
        let mut paragraph = builder.build();
        paragraph.layout(width);
        Ok(Self {
            paragraph,
            index_map: TextIndexMap::from_text(&text),
        })
    }

    pub fn text_index_map(&self) -> &TextIndexMap {
        &self.index_map
    }

    /// Returns paragraph width and height in logical pixels.
    pub fn size(&self) -> (f32, f32) {
        let width = if self.paragraph.line_number() == 0 {
            0.0
        } else {
            self.paragraph.longest_line()
        };
        (width, self.paragraph.height())
    }

    pub fn line_count(&self) -> usize {
        self.paragraph.line_number()
    }

    /// Draws this paragraph at its top-left origin on a host-owned canvas.
    pub fn paint(&self, canvas: &Canvas, x: f32, y: f32) {
        self.paragraph.paint(canvas, Point::new(x, y));
    }

    /// Maps a local paragraph coordinate to a core grapheme offset.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<TextHit> {
        let hit = self.glyph_hit(x, y)?;
        // Skia hits individual glyphs. A grapheme shaped from several glyphs,
        // such as a Devanagari conjunct, then yields offsets that alternate
        // inside it, so resolve within a grapheme by its midpoint instead.
        let start = hit.grapheme_offset.saturating_sub(1);
        let end = (hit.grapheme_offset + 1).min(self.index_map.grapheme_len());
        for offset in start..end {
            for rect in self.selection_rects(offset..offset + 1).ok()? {
                if x < rect.left || x > rect.right || y < rect.top || y > rect.bottom {
                    continue;
                }
                let epsilon = (rect.width() * 0.01).clamp(0.001, 0.1);
                let edge = if x < rect.center_x() {
                    rect.left + epsilon
                } else {
                    rect.right - epsilon
                };
                return self.glyph_hit(edge, rect.center_y());
            }
        }
        Some(hit)
    }

    fn glyph_hit(&self, x: f32, y: f32) -> Option<TextHit> {
        let hit: PositionWithAffinity = self
            .paragraph
            .get_glyph_position_at_coordinate(Point::new(x, y));
        let affinity = match hit.affinity {
            SkiaAffinity::Upstream => EditorAffinity::Upstream,
            SkiaAffinity::Downstream => EditorAffinity::Downstream,
        };
        let grapheme_offset = self.index_map.from_utf16(hit.position as usize, affinity)?;
        Some(TextHit {
            grapheme_offset,
            affinity,
        })
    }

    /// Returns Skia's selection/highlight rectangles for a core grapheme range.
    pub fn selection_rects(
        &self,
        grapheme_range: std::ops::Range<usize>,
    ) -> Result<Vec<Rect>, TextIndexError> {
        let utf16_range = self.index_map.range_to_utf16(grapheme_range)?;
        Ok(self
            .paragraph
            .get_rects_for_range(utf16_range, RectHeightStyle::Max, RectWidthStyle::Tight)
            .into_iter()
            .map(|text_box| text_box.rect)
            .collect())
    }
}

thread_local! {
    // FontMgr is thread-bound; reuse it safely within a render worker rather
    // than enumerating system fonts for every paragraph of every request.
    static SYSTEM_FONTS: FontMgr = FontMgr::default();
    static SHARED_FONTS: FontMgr = {
        let manager = FontMgr::default();
        let mut provider = TypefaceFontProvider::new();
        for (_, bytes) in BUNDLED_FONTS {
            let face = manager.new_from_data(Data::new_copy(bytes), None)
                .expect("bundled font must decode");
            provider.register_typeface(face, None);
        }
        provider.into()
    };
}

fn paragraph_config(
    width: f32,
    font_size: f32,
) -> Result<(ParagraphStyle, FontCollection), SkiaLayoutError> {
    if !width.is_finite() || width <= 0.0 {
        return Err(SkiaLayoutError::InvalidWidth);
    }
    if !font_size.is_finite() || font_size <= 0.0 {
        return Err(SkiaLayoutError::InvalidFontSize);
    }

    let mut fonts = FontCollection::new();
    fonts.set_default_font_manager(SYSTEM_FONTS.with(Clone::clone), None);
    fonts.set_asset_font_manager(SHARED_FONTS.with(Clone::clone));
    let text_style = text_style(font_size, &[]);
    let mut paragraph_style = ParagraphStyle::new();
    paragraph_style.set_text_style(&text_style);
    paragraph_style.turn_hinting_off();
    paragraph_style.set_apply_rounding_hack(false);
    Ok((paragraph_style, fonts))
}

fn text_style(font_size: f32, marks: &[Mark]) -> TextStyle {
    let bold = marks.iter().any(|mark| matches!(mark, Mark::Bold));
    let italic = marks.iter().any(|mark| matches!(mark, Mark::Italic));
    let mut style = TextStyle::new();
    style.set_font_size(font_size);
    style.set_color(Color::BLACK);
    // Scripts absent from Liberation use bundled Noto so the browser's DOM
    // text, and with it Android's native caret handle, matches Skia.
    style.set_font_families(&[
        "Liberation Sans",
        "Noto Sans Arabic",
        "Noto Sans Devanagari",
        "Arial",
    ]);
    style.set_font_style(match (bold, italic) {
        (true, true) => FontStyle::bold_italic(),
        (true, false) => FontStyle::bold(),
        (false, true) => FontStyle::italic(),
        (false, false) => FontStyle::normal(),
    });

    let mut decoration = skia_safe::textlayout::TextDecoration::NO_DECORATION;
    if marks
        .iter()
        .any(|mark| matches!(mark, Mark::Underline | Mark::Link { .. }))
    {
        decoration |= skia_safe::textlayout::TextDecoration::UNDERLINE;
    }
    if marks.iter().any(|mark| matches!(mark, Mark::Strike)) {
        decoration |= skia_safe::textlayout::TextDecoration::LINE_THROUGH;
    }
    style.set_decoration_type(decoration);

    if marks.iter().any(|mark| matches!(mark, Mark::Code)) {
        // Skia resolves installed family names, not CSS generic families.
        style.set_font_families(&[
            "Liberation Mono",
            "Noto Sans Arabic",
            "Noto Sans Devanagari",
            "Menlo",
            "Consolas",
            "DejaVu Sans Mono",
            "Courier New",
        ]);
        style.set_color(Color::from_rgb(157, 45, 83));
        let mut background = Paint::default();
        background.set_color(Color::from_rgb(245, 239, 243));
        style.set_background_paint(&background);
    }
    style
}

impl ParagraphLayout for SkiaParagraph {
    fn size(&self) -> LayoutSize {
        let (width, height) = SkiaParagraph::size(self);
        LayoutSize { width, height }
    }

    fn line_count(&self) -> usize {
        SkiaParagraph::line_count(self)
    }

    fn hit_test(&self, x: f32, y: f32) -> Option<TextHit> {
        SkiaParagraph::hit_test(self, x, y)
    }

    fn selection_rects(
        &self,
        grapheme_range: std::ops::Range<usize>,
    ) -> Result<Vec<LayoutRect>, TextIndexError> {
        SkiaParagraph::selection_rects(self, grapheme_range).map(|rects| {
            rects
                .into_iter()
                .map(|rect| LayoutRect {
                    x: rect.left(),
                    y: rect.top(),
                    width: rect.width(),
                    height: rect.height(),
                })
                .collect()
        })
    }

    fn text_len(&self) -> usize {
        self.index_map.grapheme_len()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SkiaRenderer;

impl LayoutBackend for SkiaRenderer {
    type Layout = SkiaParagraph;
    type Error = SkiaLayoutError;

    fn layout_block(
        &self,
        block: &Block,
        width: f32,
        font_size: f32,
    ) -> Result<Self::Layout, Self::Error> {
        SkiaParagraph::from_block(block, width, font_size)
    }
}

impl Renderer for SkiaRenderer {
    type Target = Canvas;

    fn draw(&self, layout: &Self::Layout, target: &Self::Target, origin: LayoutPoint) {
        layout.paint(target, origin.x, origin.y);
    }
}

/// Complete document layout and drawing helpers for interactive hosts.
pub struct SkiaDocumentLayout {
    document: DocumentLayout<SkiaParagraph>,
    marker_layouts: Vec<Option<SkiaParagraph>>,
    font_size: f32,
}

impl SkiaDocumentLayout {
    pub fn new(
        document: &openrich_document::Document,
        width: f32,
        font_size: f32,
    ) -> Result<Self, SkiaLayoutError> {
        let layout = DocumentLayout::new(document, &SkiaRenderer, width, font_size)?;
        let marker_layouts = layout
            .entries
            .iter()
            .map(|entry| {
                entry
                    .marker
                    .as_ref()
                    .filter(|marker| !matches!(marker.as_str(), "☐" | "☑"))
                    .map(|marker| SkiaParagraph::new(marker, font_size * 1.5, font_size))
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            document: layout,
            marker_layouts,
            font_size,
        })
    }

    pub fn size(&self) -> LayoutSize {
        self.document.size
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<openrich_render::DocumentHit> {
        self.document.hit_test(x, y)
    }

    /// Positioned paragraphs for hosts that cache interaction geometry locally.
    pub fn document_layout(&self) -> &DocumentLayout<SkiaParagraph> {
        &self.document
    }

    pub fn hit_test_nearest(&self, x: f32, y: f32) -> Option<openrich_render::DocumentHit> {
        self.document.hit_test_nearest(x, y)
    }

    pub fn paint(
        &self,
        canvas: &Canvas,
        origin: LayoutPoint,
        selection: Option<&openrich_document::Selection>,
        focused: bool,
    ) {
        if let Some(selection) = selection.filter(|selection| !selection.is_collapsed()) {
            let mut paint = Paint::default();
            paint.set_color(Color::from_argb(0x66, 0x35, 0x7A, 0xD8));
            if let Ok(rects) = self.document.selection_rects(selection) {
                for rect in rects {
                    canvas.draw_rect(
                        Rect::from_xywh(
                            origin.x + rect.x,
                            origin.y + rect.y,
                            rect.width,
                            rect.height,
                        ),
                        &paint,
                    );
                }
            }
        }
        for (index, entry) in self.document.entries.iter().enumerate() {
            let x = origin.x + entry.x;
            let y = origin.y + entry.y;
            match entry.kind {
                openrich_render::DocumentEntryKind::HorizontalRule => {
                    let mut paint = Paint::default();
                    paint.set_color(Color::from_argb(0xFF, 0x99, 0x99, 0x99));
                    paint.set_stroke_width(1.0);
                    canvas.draw_line(
                        Point::new(x, y + entry.height * 0.5),
                        Point::new(x + entry.width, y + entry.height * 0.5),
                        &paint,
                    );
                }
                _ => {
                    if let Some(layout) = &entry.layout {
                        layout.paint(canvas, x, y);
                    }
                    if matches!(entry.marker.as_deref(), Some("☐" | "☑")) {
                        let checked = entry.marker.as_deref() == Some("☑");
                        let unit = self.font_size;
                        let left = x - unit * 1.5;
                        let top = y + unit * 0.18;
                        let rect = Rect::from_xywh(left, top, unit, unit);
                        let mut paint = Paint::default();
                        paint.set_anti_alias(true);
                        paint.set_color(if checked {
                            Color::from_rgb(76, 97, 206)
                        } else {
                            Color::WHITE
                        });
                        canvas.draw_round_rect(rect, unit * 0.22, unit * 0.22, &paint);
                        paint.set_style(skia_safe::paint::Style::Stroke);
                        paint.set_stroke_width(unit * 0.09);
                        paint.set_color(if checked {
                            Color::from_rgb(76, 97, 206)
                        } else {
                            Color::from_rgb(139, 151, 170)
                        });
                        canvas.draw_round_rect(rect, unit * 0.22, unit * 0.22, &paint);
                        if checked {
                            paint.set_color(Color::WHITE);
                            paint.set_stroke_width(unit * 0.12);
                            paint.set_stroke_cap(skia_safe::paint::Cap::Round);
                            let joint = Point::new(left + unit * 0.43, top + unit * 0.71);
                            canvas.draw_line(
                                Point::new(left + unit * 0.23, top + unit * 0.51),
                                joint,
                                &paint,
                            );
                            canvas.draw_line(
                                joint,
                                Point::new(left + unit * 0.78, top + unit * 0.30),
                                &paint,
                            );
                        }
                    } else if let Some(marker) = &self.marker_layouts[index] {
                        marker.paint(canvas, x - self.font_size * 1.5, y);
                    }
                }
            }
        }
        if focused {
            if let Some(selection) = selection.filter(|selection| selection.is_collapsed()) {
                if let Ok(Some(rect)) = self.document.caret_rect(&selection.focus) {
                    let mut paint = Paint::default();
                    paint.set_color(Color::from_argb(0xFF, 0x18, 0x54, 0xB8));
                    canvas.draw_rect(
                        Rect::from_xywh(origin.x + rect.x, origin.y + rect.y, 1.5, rect.height),
                        &paint,
                    );
                }
            }
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SkiaLayoutError {
    #[error("paragraph width must be finite and greater than zero")]
    InvalidWidth,
    #[error("font size must be finite and greater than zero")]
    InvalidFontSize,
    #[error("Skia paragraph backend currently supports paragraph and heading blocks")]
    UnsupportedBlock,
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrich_document::{
        DOCUMENT_SCHEMA_VERSION, Document, Inline, ListItem, NodeId, Position, Selection,
    };
    use openrich_render::{DocumentEntryKind, DocumentLayout};

    #[test]
    fn hit_testing_inside_a_multi_glyph_grapheme_is_monotonic() {
        // Noto shapes "न्दी" from several glyphs; offsets must not step back.
        let paragraph = SkiaParagraph::new("हिन्दी", 400.0, 16.0).unwrap();
        let (width, height) = paragraph.size();
        let mut previous = 0;
        for step in 0..=400 {
            let x = width * step as f32 / 400.0;
            let offset = paragraph.hit_test(x, height / 2.0).unwrap().grapheme_offset;
            assert!(offset >= previous, "offset {offset} after {previous} at x={x}");
            previous = offset;
        }
        assert_eq!(previous, paragraph.text_index_map().grapheme_len());
    }

    #[test]
    fn paragraph_layout_hit_testing_and_selection_geometry_work() {
        let paragraph = SkiaParagraph::new("Hello 🙂 world", 70.0, 16.0).unwrap();
        let (width, height) = paragraph.size();

        assert!(width > 0.0);
        assert!(height > 0.0);
        assert!(paragraph.line_count() >= 1);
        let hit = paragraph.hit_test(4.0, 8.0).unwrap();
        assert!(hit.grapheme_offset <= paragraph.text_index_map().grapheme_len());
        assert!(!paragraph.selection_rects(0..5).unwrap().is_empty());
    }

    #[test]
    fn inline_marks_map_to_skia_font_and_decoration_styles() {
        let style = text_style(
            16.0,
            &[
                Mark::Bold,
                Mark::Italic,
                Mark::Underline,
                Mark::Strike,
                Mark::Code,
            ],
        );

        assert_eq!(style.font_style(), FontStyle::bold_italic());
        assert_eq!(
            style.decoration_type(),
            skia_safe::textlayout::TextDecoration::UNDERLINE
                | skia_safe::textlayout::TextDecoration::LINE_THROUGH
        );
        assert_eq!(style.font_families().iter().next(), Some("Liberation Mono"));
    }

    #[test]
    fn document_runs_keep_the_same_index_map_as_flattened_text() {
        let block = Block::Paragraph {
            id: NodeId::new(),
            content: vec![
                openrich_document::Inline::text("Hi "),
                openrich_document::Inline::Text {
                    text: "🙂!".into(),
                    marks: vec![Mark::Bold],
                },
            ],
        };
        let paragraph = SkiaParagraph::from_block(&block, 160.0, 16.0).unwrap();

        assert_eq!(paragraph.text_index_map().grapheme_len(), 5);
        assert_eq!(paragraph.text_index_map().utf16_len(), 6);
        assert_eq!(paragraph.selection_rects(3..4).unwrap().len(), 1);
    }

    #[test]
    fn document_layout_positions_blocks_and_maps_selection_and_caret() {
        let document = Document {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![
                Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text("first paragraph")],
                },
                Block::BulletList {
                    id: NodeId::new(),
                    items: vec![ListItem::new(vec![Block::paragraph()])],
                },
                Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text("second paragraph")],
                },
                Block::HorizontalRule { id: NodeId::new() },
            ],
        };
        let layout = DocumentLayout::new(&document, &SkiaRenderer, 320.0, 16.0).unwrap();

        assert_eq!(layout.entries.len(), 4);
        assert!(layout.entries[0].y < layout.entries[1].y);
        assert!(layout.entries[1].x > layout.entries[0].x);
        assert_eq!(layout.entries[3].kind, DocumentEntryKind::HorizontalRule);
        assert!(layout.size.height > 60.0);

        let selection = Selection {
            anchor: Position {
                block_index: 0,
                offset: 1,
                affinity: EditorAffinity::Downstream,
            },
            focus: Position {
                block_index: 2,
                offset: 3,
                affinity: EditorAffinity::Downstream,
            },
        };
        let rects = layout.selection_rects(&selection).unwrap();
        assert!(rects.len() >= 2);
        assert!(rects.first().unwrap().y < rects.last().unwrap().y);
        let caret = layout.caret_rect(&selection.focus).unwrap().unwrap();
        assert!(caret.y > rects[0].y);

        let hit = layout
            .hit_test(layout.entries[0].x + 1.0, layout.entries[0].y + 4.0)
            .unwrap();
        assert_eq!(hit.top_level_index, 0);
    }

    #[test]
    fn pointer_hits_use_visual_graphemes_and_snap_outside_text_at_retina_scales() {
        let document = Document {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![
                Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text("A👩‍💻B wrapping words to fill several lines")],
                },
                Block::Paragraph {
                    id: NodeId::new(),
                    content: vec![Inline::text("last")],
                },
            ],
        };
        for scale in [1.0, 2.0, 3.0] {
            let layout =
                DocumentLayout::new(&document, &SkiaRenderer, 120.0 * scale, 16.0 * scale).unwrap();
            let entry = &layout.entries[0];
            assert!(entry.layout.as_ref().unwrap().line_count() > 1);
            let emoji = entry
                .layout
                .as_ref()
                .unwrap()
                .selection_rects(1..2)
                .unwrap()[0];
            let hit = layout
                .hit_test_nearest(
                    emoji.x() + emoji.width() * 0.1,
                    emoji.y() + emoji.height() * 0.5,
                )
                .unwrap();
            assert_eq!(hit.top_level_index, 0);
            assert_eq!(hit.hit.grapheme_offset, 1);
            let before = layout.hit_test_nearest(-100.0, -100.0).unwrap();
            assert_eq!((before.top_level_index, before.hit.grapheme_offset), (0, 0));
            let after = layout.hit_test_nearest(10000.0, 10000.0).unwrap();
            assert_eq!((after.top_level_index, after.hit.grapheme_offset), (1, 4));
        }
    }

    #[test]
    fn lays_out_mixed_rtl_and_emoji_text() {
        let paragraph = SkiaParagraph::new("עברית العربية English 👩‍💻", 260.0, 18.0).unwrap();
        assert!(paragraph.line_count() >= 1);
        assert!(paragraph.size().1 > 0.0);
        assert!(!paragraph.selection_rects(0..4).unwrap().is_empty());
        let hit = paragraph.hit_test(10.0, 9.0).unwrap();
        assert!(hit.grapheme_offset <= paragraph.text_index_map().grapheme_len());
    }

    #[test]
    fn software_surface_produces_a_png_document_frame() {
        let document = Document {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![Block::Paragraph {
                id: NodeId::new(),
                content: vec![Inline::text("OpenRich raster smoke")],
            }],
        };
        let layout = SkiaDocumentLayout::new(&document, 320.0, 18.0).unwrap();
        let mut surface = skia_safe::surfaces::raster_n32_premul((360, 160)).unwrap();
        surface.canvas().clear(Color::WHITE);
        layout.paint(
            surface.canvas(),
            LayoutPoint { x: 16.0, y: 16.0 },
            None,
            false,
        );
        #[allow(deprecated)]
        let png = surface
            .image_snapshot()
            .encode_to_data(skia_safe::EncodedImageFormat::PNG)
            .unwrap();
        assert!(png.as_bytes().starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png.size() > 100);
    }
    #[test]
    fn alignment_and_checklist_geometry_remain_consistent_at_retina_scales() {
        use openrich_document::{BlockFormat, ListStyle, TextAlignment};
        for scale in [1.0, 2.0, 3.0] {
            let mut previous_x = -1.0;
            for alignment in [
                TextAlignment::Left,
                TextAlignment::Center,
                TextAlignment::Right,
            ] {
                let block = Block::Formatted {
                    id: NodeId::new(),
                    content: vec![Inline::Text {
                        text: "A👩‍💻B".into(),
                        marks: vec![Mark::Bold],
                    }],
                    format: BlockFormat {
                        alignment,
                        list: ListStyle::Checkbox,
                        ..Default::default()
                    },
                };
                let document = Document {
                    schema_version: DOCUMENT_SCHEMA_VERSION,
                    blocks: vec![block],
                };
                let layout =
                    SkiaDocumentLayout::new(&document, 300.0 * scale, 16.0 * scale).unwrap();
                let entry = &layout.document.entries[0];
                assert_eq!(entry.marker.as_deref(), Some("☐"));
                assert!(
                    layout.marker_layouts[0].is_none(),
                    "checkbox must be vector drawn, not a font glyph"
                );
                let paragraph = entry.layout.as_ref().unwrap();
                let rect = paragraph.selection_rects(1..2).unwrap()[0];
                assert!(rect.left > previous_x);
                previous_x = rect.left;
                assert_eq!(
                    paragraph
                        .hit_test(
                            rect.left + rect.width() * 0.1,
                            rect.y() + rect.height() * 0.5
                        )
                        .unwrap()
                        .grapheme_offset,
                    1
                );
                assert_eq!(
                    paragraph
                        .hit_test(
                            rect.right - rect.width() * 0.1,
                            rect.y() + rect.height() * 0.5
                        )
                        .unwrap()
                        .grapheme_offset,
                    2
                );
                assert!(paragraph.caret_rect(1).unwrap().is_some());
            }
        }
    }
}
