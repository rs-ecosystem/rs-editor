//! Lays out a whole document: block structure, list markers, and
//! position ↔ geometry queries across blocks.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use openrich_model::Node;
use openrich_text::Affinity;
use openrich_theme::Theme;
use skia_safe::textlayout::{
    FontCollection, ParagraphBuilder, ParagraphStyle, PlaceholderAlignment, PlaceholderStyle,
    TextAlign, TextBaseline,
};

use crate::fonts::font_collection;
use crate::geometry::Rect;
use crate::style::{BlockStyle, base_text_style, block_style, run_style, strut};
use crate::text::{CaretStop, TextLayout};

/// A laid-out textblock in document coordinates.
#[derive(Debug, Clone)]
pub struct TextBox {
    /// Position before the block node.
    pub node_pos: usize,
    /// First content position (`node_pos + 1`).
    pub pos: usize,
    pub node: Node,
    /// Where the paragraph is drawn (its own size).
    pub rect: Rect,
    pub layout: Rc<TextLayout>,
}

impl TextBox {
    /// Last content position.
    pub fn end(&self) -> usize {
        self.pos + self.layout.len()
    }
}

/// Bullet shapes by nesting depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulletStyle {
    Disc,
    Circle,
    Square,
}

/// Non-text visuals produced by layout.
#[derive(Debug, Clone)]
pub enum Decoration {
    Bullet {
        cx: f32,
        cy: f32,
        radius: f32,
        style: BulletStyle,
    },
    /// An ordered-list number drawn with `layout` at (`x`, `y`).
    Number {
        x: f32,
        y: f32,
        layout: Rc<TextLayout>,
    },
    /// A task checkbox; `pos` is the position before the task item.
    Checkbox {
        rect: Rect,
        checked: bool,
        pos: usize,
    },
    QuoteBar {
        rect: Rect,
    },
    CodeBackground {
        rect: Rect,
    },
    Rule {
        rect: Rect,
    },
    Image {
        rect: Rect,
        src: Option<String>,
        alt: Option<String>,
    },
}

/// Every block node with its rectangle (for node selection and handles).
#[derive(Debug, Clone)]
pub struct BlockBox {
    pub pos: usize,
    pub node: Node,
    pub rect: Rect,
    pub depth: usize,
}

/// What a point hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A text position.
    Text { pos: usize, affinity: Affinity },
    /// A leaf block (rule, image) starting at `pos`.
    Node { pos: usize },
}

/// Horizontal caret movement direction on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualDirection {
    Left,
    Right,
}

/// The result of laying out a document at one width.
#[derive(Debug, Clone)]
pub struct DocumentLayout {
    pub width: f32,
    pub height: f32,
    pub texts: Vec<TextBox>,
    pub decorations: Vec<Decoration>,
    pub blocks: Vec<BlockBox>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    node: Node,
    layout: Rc<TextLayout>,
}

/// Lays out documents, reusing paragraphs of unchanged blocks.
pub struct LayoutEngine {
    fonts: FontCollection,
    theme: Theme,
    cache: HashMap<(usize, u32, bool), CacheEntry>,
}

impl fmt::Debug for LayoutEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LayoutEngine")
            .field("cached_blocks", &self.cache.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
struct Ctx {
    italic: bool,
    list_depth: usize,
    depth: usize,
}

struct Builder<'a> {
    engine: &'a mut LayoutEngine,
    next_cache: HashMap<(usize, u32, bool), CacheEntry>,
    out: DocumentLayout,
}

impl LayoutEngine {
    /// `system_fallback` lets the platform's fonts cover emoji and scripts
    /// missing from the bundle; turn it off for deterministic tests.
    pub fn new(theme: Theme, system_fallback: bool) -> Self {
        Self {
            fonts: font_collection(system_fallback),
            theme,
            cache: HashMap::new(),
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Changes the theme; every block is laid out again.
    pub fn set_theme(&mut self, theme: Theme) {
        if theme != self.theme {
            self.theme = theme;
            self.cache.clear();
        }
    }

    pub fn cached_blocks(&self) -> usize {
        self.cache.len()
    }

    /// Lays out `doc` at `width` logical pixels.
    pub fn layout(&mut self, doc: &Node, width: f32) -> DocumentLayout {
        let mut builder = Builder {
            engine: self,
            next_cache: HashMap::new(),
            out: DocumentLayout {
                width,
                height: 0.0,
                texts: Vec::new(),
                decorations: Vec::new(),
                blocks: Vec::new(),
            },
        };
        let ctx = Ctx {
            italic: false,
            list_depth: 0,
            depth: 0,
        };
        let height = builder.stack(doc, 0, 0.0, 0.0, width, ctx, false);
        let Builder {
            engine,
            next_cache,
            mut out,
        } = builder;
        engine.cache = next_cache;
        out.height = height;
        out
    }

    fn text_layout(&self, node: &Node, width: f32, italic: bool) -> TextLayout {
        let theme = &self.theme;
        let block: BlockStyle = block_style(theme, node, italic);
        let base = base_text_style(theme, &block);
        let mut paragraph_style = ParagraphStyle::new();
        paragraph_style.set_text_style(&base);
        paragraph_style.set_strut_style(strut(theme, &block));
        paragraph_style.set_text_align(block.align);
        paragraph_style.set_apply_rounding_hack(false);
        paragraph_style.turn_hinting_off();
        let mut builder = ParagraphBuilder::new(&paragraph_style, self.fonts.clone());
        let mut text = String::new();
        for child in node.children() {
            let style = run_style(theme, &block, child.marks());
            if let Some(run) = child.text() {
                builder.push_style(&style);
                builder.add_text(run);
                builder.pop();
                text.push_str(run);
            } else if child.node_type().spec().leaf_text.as_deref() == Some("\n") {
                builder.push_style(&style);
                builder.add_text("\n");
                builder.pop();
                text.push('\n');
            } else {
                // Inline atoms (images, mentions) take one position each.
                let size = block.size * 1.25;
                builder.push_style(&style);
                builder.add_placeholder(&PlaceholderStyle::new(
                    size,
                    size,
                    PlaceholderAlignment::Baseline,
                    TextBaseline::Alphabetic,
                    size * 0.8,
                ));
                builder.pop();
                text.push('\u{fffc}');
            }
        }
        let mut paragraph = builder.build();
        paragraph.layout(width.max(1.0));
        TextLayout::new(
            paragraph,
            text,
            width,
            block.align,
            block.line_height,
            block.size * 1.3,
        )
    }

    /// A one-line label (list numbers) in the paragraph style.
    fn label(&self, text: &str) -> TextLayout {
        let theme = &self.theme;
        let block = BlockStyle {
            size: theme.typography.paragraph.size,
            line_height: theme.typography.paragraph.line_height,
            weight: theme.typography.paragraph.weight,
            letter_spacing_em: 0.0,
            mono: false,
            italic: false,
            color: theme.palette.foreground,
            align: TextAlign::Right,
        };
        let mut paragraph_style = ParagraphStyle::new();
        let style = base_text_style(theme, &block);
        paragraph_style.set_text_style(&style);
        paragraph_style.set_strut_style(strut(theme, &block));
        paragraph_style.turn_hinting_off();
        let mut builder = ParagraphBuilder::new(&paragraph_style, self.fonts.clone());
        builder.push_style(&style);
        builder.add_text(text);
        let mut paragraph = builder.build();
        paragraph.layout(1_000.0);
        let width = paragraph.max_intrinsic_width().ceil();
        paragraph.layout(width + 1.0);
        TextLayout::new(
            paragraph,
            text.to_owned(),
            width,
            TextAlign::Left,
            block.line_height,
            block.size * 1.3,
        )
    }
}

fn is_list(node: &Node) -> bool {
    node.node_type().is_in_group("list")
}

impl Builder<'_> {
    fn theme(&self) -> &Theme {
        &self.engine.theme
    }

    fn gap(&self, previous: Option<&Node>, node: &Node, in_item: bool) -> f32 {
        let spacing = &self.theme().spacing;
        match previous {
            None => 0.0,
            Some(_) if in_item => spacing.list_item_gap,
            Some(_) if node.type_name() == "heading" => spacing.block_gap + spacing.heading_gap,
            Some(_) => spacing.block_gap,
        }
    }

    /// Lays out the children of `parent` (content starting at `content_pos`)
    /// top to bottom; returns their total height.
    #[allow(clippy::too_many_arguments)]
    fn stack(
        &mut self,
        parent: &Node,
        content_pos: usize,
        x: f32,
        y: f32,
        width: f32,
        ctx: Ctx,
        in_item: bool,
    ) -> f32 {
        let mut cy = y;
        let mut pos = content_pos;
        let mut previous: Option<&Node> = None;
        for child in parent.children() {
            cy += self.gap(previous, child, in_item);
            cy += self.block(child, pos, x, cy, width, ctx);
            pos += child.node_size();
            previous = Some(child);
        }
        cy - y
    }

    fn text(&mut self, node: &Node, pos: usize, x: f32, y: f32, width: f32, ctx: Ctx) -> f32 {
        let key = (node.identity(), width.to_bits(), ctx.italic);
        let layout = match self
            .engine
            .cache
            .get(&key)
            .or_else(|| self.next_cache.get(&key))
        {
            Some(entry) if entry.node.ptr_eq(node) => entry.layout.clone(),
            _ => Rc::new(self.engine.text_layout(node, width, ctx.italic)),
        };
        self.next_cache.insert(
            key,
            CacheEntry {
                node: node.clone(),
                layout: layout.clone(),
            },
        );
        let height = layout.height();
        self.out.texts.push(TextBox {
            node_pos: pos,
            pos: pos + 1,
            node: node.clone(),
            rect: Rect::new(x, y, width, height),
            layout,
        });
        height
    }

    fn block(&mut self, node: &Node, pos: usize, x: f32, y: f32, width: f32, ctx: Ctx) -> f32 {
        let block_index = self.out.blocks.len();
        self.out.blocks.push(BlockBox {
            pos,
            node: node.clone(),
            rect: Rect::new(x, y, width, 0.0),
            depth: ctx.depth,
        });
        let inner = Ctx {
            depth: ctx.depth + 1,
            ..ctx
        };
        let height = if node.is_textblock() {
            if node.node_type().spec().code {
                let pad = self.theme().spacing.code_block_padding;
                self.out.decorations.push(Decoration::CodeBackground {
                    rect: Rect::default(),
                });
                let index = self.out.decorations.len() - 1;
                let text_height = self.text(
                    node,
                    pos,
                    x + pad,
                    y + pad,
                    (width - 2.0 * pad).max(1.0),
                    inner,
                );
                let total = text_height + 2.0 * pad;
                self.out.decorations[index] = Decoration::CodeBackground {
                    rect: Rect::new(x, y, width, total),
                };
                total
            } else {
                self.text(node, pos, x, y, width, ctx)
            }
        } else if node.type_name() == "blockquote" {
            let spacing = self.theme().spacing;
            let quoted = Ctx {
                italic: true,
                ..inner
            };
            let height = self.stack(
                node,
                pos + 1,
                x + spacing.quote_indent,
                y,
                width - spacing.quote_indent,
                quoted,
                false,
            );
            self.out.decorations.push(Decoration::QuoteBar {
                rect: Rect::new(x, y, spacing.quote_border, height),
            });
            height
        } else if is_list(node) {
            self.list(node, pos, x, y, width, inner)
        } else if node.is_leaf() {
            self.leaf_block(node, x, y, width)
        } else {
            self.stack(node, pos + 1, x, y, width, inner, false)
        };
        self.out.blocks[block_index].rect.height = height;
        height
    }

    fn list(&mut self, list: &Node, pos: usize, x: f32, y: f32, width: f32, ctx: Ctx) -> f32 {
        let spacing = self.theme().spacing;
        let indent = spacing.list_indent;
        let start = list.attr("start").and_then(|v| v.as_u64()).unwrap_or(1);
        let item_ctx = Ctx {
            list_depth: ctx.list_depth + 1,
            ..ctx
        };
        let mut cy = y;
        let mut item_pos = pos + 1;
        for (index, item) in list.children().enumerate() {
            if index > 0 {
                cy += spacing.list_item_gap;
            }
            let first_text = self.out.texts.len();
            let block_index = self.out.blocks.len();
            self.out.blocks.push(BlockBox {
                pos: item_pos,
                node: item.clone(),
                rect: Rect::new(x, cy, width, 0.0),
                depth: ctx.depth,
            });
            let height = self.stack(
                item,
                item_pos + 1,
                x + indent,
                cy,
                width - indent,
                Ctx {
                    depth: ctx.depth + 1,
                    ..item_ctx
                },
                true,
            );
            self.out.blocks[block_index].rect.height = height;
            // Align the marker with the item's first line.
            let (line_center, baseline) = match self.out.texts.get(first_text) {
                Some(text) => {
                    let line = text.layout.lines()[0];
                    (
                        text.rect.y + line.top + line.height / 2.0,
                        text.rect.y + line.baseline,
                    )
                }
                None => (cy + spacing.list_item_gap, cy + spacing.list_item_gap),
            };
            match list.type_name() {
                "orderedList" => {
                    let label = Rc::new(self.engine.label(&format!("{}.", start + index as u64)));
                    let label_y = baseline - label.lines()[0].baseline;
                    self.out.decorations.push(Decoration::Number {
                        x: x + indent - 6.0 - label.width(),
                        y: label_y,
                        layout: label,
                    });
                }
                "taskList" => {
                    let size = spacing.checkbox_size;
                    self.out.decorations.push(Decoration::Checkbox {
                        rect: Rect::new(x, line_center - size / 2.0, size, size),
                        checked: item
                            .attr("checked")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        pos: item_pos,
                    });
                }
                _ => {
                    let style = match ctx.list_depth % 3 {
                        0 => BulletStyle::Disc,
                        1 => BulletStyle::Circle,
                        _ => BulletStyle::Square,
                    };
                    self.out.decorations.push(Decoration::Bullet {
                        cx: x + indent - 13.0,
                        cy: line_center,
                        radius: 2.75,
                        style,
                    });
                }
            }
            cy += height;
            item_pos += item.node_size();
        }
        cy - y
    }

    fn leaf_block(&mut self, node: &Node, x: f32, y: f32, width: f32) -> f32 {
        match node.type_name() {
            "horizontalRule" => {
                let height = 24.0;
                self.out.decorations.push(Decoration::Rule {
                    rect: Rect::new(x, y + height / 2.0, width, 1.0),
                });
                height
            }
            "image" => {
                let attr = |name: &str| node.attr(name).and_then(|v| v.as_f64()).map(|v| v as f32);
                let (w, h) = match (attr("width"), attr("height")) {
                    (Some(w), Some(h)) if w > 0.0 && h > 0.0 => {
                        let w = w.min(width);
                        (w, w * h / attr("width").unwrap_or(w))
                    }
                    _ => (width.min(640.0), width.min(640.0) * 9.0 / 16.0),
                };
                let text = |name: &str| node.attr(name).and_then(|v| v.as_str()).map(str::to_owned);
                self.out.decorations.push(Decoration::Image {
                    rect: Rect::new(x, y, w, h),
                    src: text("src"),
                    alt: text("alt"),
                });
                h
            }
            _ => 24.0,
        }
    }
}

impl DocumentLayout {
    /// The text box whose content range contains `pos`.
    pub fn text_at(&self, pos: usize) -> Option<&TextBox> {
        let index = self.texts.partition_point(|text| text.end() < pos);
        self.texts.get(index).filter(|text| text.pos <= pos)
    }

    fn text_index(&self, pos: usize) -> Option<usize> {
        let index = self.texts.partition_point(|text| text.end() < pos);
        self.texts
            .get(index)
            .filter(|text| text.pos <= pos)
            .map(|_| index)
    }

    /// The caret rectangle for a text position.
    pub fn caret_rect(&self, pos: usize, affinity: Affinity, caret_width: f32) -> Option<Rect> {
        let text = self.text_at(pos)?;
        let caret = text.layout.caret(pos - text.pos, affinity);
        Some(Rect::new(
            text.rect.x + caret.x - caret_width / 2.0,
            text.rect.y + caret.y,
            caret_width,
            caret.height,
        ))
    }

    /// The position under a point (clamped to the nearest text).
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        for block in &self.blocks {
            if block.node.is_leaf() && block.node.is_block() && block.rect.contains(x, y) {
                return Some(Hit::Node { pos: block.pos });
            }
        }
        let text = self
            .texts
            .iter()
            .filter(|t| y >= t.rect.y && y <= t.rect.bottom())
            .min_by(|a, b| a.rect.distance(x, y).total_cmp(&b.rect.distance(x, y)))
            .or_else(|| {
                self.texts
                    .iter()
                    .min_by(|a, b| a.rect.distance(x, y).total_cmp(&b.rect.distance(x, y)))
            })?;
        let (offset, affinity) = text.layout.offset_at(x - text.rect.x, y - text.rect.y);
        Some(Hit::Text {
            pos: text.pos + offset,
            affinity,
        })
    }

    /// Highlight rectangles for the text and leaf blocks in `from..to`.
    pub fn selection_rects(&self, from: usize, to: usize) -> Vec<Rect> {
        let mut rects = Vec::new();
        if from >= to {
            return rects;
        }
        for text in &self.texts {
            if text.end() < from || text.pos > to {
                continue;
            }
            let start = from.max(text.pos) - text.pos;
            let end = to.min(text.end()) - text.pos;
            rects.extend(
                text.layout
                    .selection_rects(start..end)
                    .into_iter()
                    .map(|r| r.translate(text.rect.x, text.rect.y)),
            );
            // The selection continues past this block: show the line break.
            if to > text.end() && from <= text.end() {
                let line = *text.layout.lines().last().unwrap_or(&crate::text::Line {
                    start: 0,
                    end: 0,
                    end_with_break: 0,
                    top: 0.0,
                    height: text.rect.height,
                    baseline: 0.0,
                    left: 0.0,
                    width: 0.0,
                });
                let caret = text.layout.caret(text.layout.len(), Affinity::Upstream);
                rects.push(Rect::new(
                    text.rect.x + caret.x,
                    text.rect.y + line.top,
                    6.0,
                    line.height,
                ));
            }
        }
        for block in &self.blocks {
            if block.node.is_leaf() && block.node.is_block() && block.pos >= from && block.pos < to
            {
                rects.push(block.rect);
            }
        }
        rects
    }

    /// Start or end of the visual line at `pos` (Home/End).
    pub fn line_edge(
        &self,
        pos: usize,
        affinity: Affinity,
        end: bool,
    ) -> Option<(usize, Affinity)> {
        let text = self.text_at(pos)?;
        let line = text.layout.lines()[text.layout.line_index(pos - text.pos, affinity)];
        Some(if end {
            let wrapped = line.end == line.end_with_break && line.end < text.layout.len();
            (
                text.pos + line.end,
                if wrapped {
                    Affinity::Upstream
                } else {
                    Affinity::Downstream
                },
            )
        } else {
            (text.pos + line.start, Affinity::Downstream)
        })
    }

    /// The position one line up (`up`) or down at horizontal `goal_x`
    /// (document coordinates), or `None` at the first/last line.
    pub fn move_vertical(
        &self,
        pos: usize,
        affinity: Affinity,
        up: bool,
        goal_x: f32,
    ) -> Option<(usize, Affinity)> {
        let index = self.text_index(pos)?;
        let text = &self.texts[index];
        let line_index = text.layout.line_index(pos - text.pos, affinity);
        let lines = text.layout.lines();
        let (target, line) = if up && line_index > 0 {
            (text, lines[line_index - 1])
        } else if !up && line_index + 1 < lines.len() {
            (text, lines[line_index + 1])
        } else if up {
            let previous = &self.texts[index.checked_sub(1)?];
            (previous, *previous.layout.lines().last()?)
        } else {
            let next = self.texts.get(index + 1)?;
            (next, next.layout.lines()[0])
        };
        let (offset, affinity) = target
            .layout
            .offset_at(goal_x - target.rect.x, line.top + line.height / 2.0);
        Some((target.pos + offset, affinity))
    }

    /// The caret stops of the line at `pos`, in document coordinates.
    pub fn line_stops(&self, pos: usize, affinity: Affinity) -> Option<(usize, Vec<CaretStop>)> {
        let index = self.text_index(pos)?;
        let text = &self.texts[index];
        let line = text.layout.line_index(pos - text.pos, affinity);
        let stops = text
            .layout
            .caret_stops(line)
            .into_iter()
            .map(|stop| CaretStop {
                x: stop.x + text.rect.x,
                offset: stop.offset + text.pos,
                affinity: stop.affinity,
            })
            .collect();
        Some((index, stops))
    }

    /// One step left or right on screen. Crossing the edge of a line moves
    /// to the adjacent line (or block) in reading order.
    pub fn move_visual(
        &self,
        pos: usize,
        affinity: Affinity,
        direction: VisualDirection,
    ) -> Option<(usize, Affinity)> {
        let index = self.text_index(pos)?;
        let text = &self.texts[index];
        let caret = self.caret_rect(pos, affinity, 0.0)?;
        let (_, stops) = self.line_stops(pos, affinity)?;
        let found = match direction {
            VisualDirection::Left => stops
                .iter()
                .rev()
                .find(|s| s.x < caret.x - 0.5 && s.offset != pos),
            VisualDirection::Right => stops
                .iter()
                .find(|s| s.x > caret.x + 0.5 && s.offset != pos),
        };
        if let Some(stop) = found {
            return Some((stop.offset, stop.affinity));
        }
        let lines = text.layout.lines();
        let line_index = text.layout.line_index(pos - text.pos, affinity);
        let forward = direction == VisualDirection::Right;
        if forward && line_index + 1 < lines.len() {
            return Some((text.pos + lines[line_index + 1].start, Affinity::Downstream));
        }
        if !forward && line_index > 0 {
            let line = lines[line_index - 1];
            return Some((text.pos + line.end, Affinity::Upstream));
        }
        if forward {
            let next = self.texts.get(index + 1)?;
            Some((next.pos, Affinity::Downstream))
        } else {
            let previous = &self.texts[index.checked_sub(1)?];
            Some((previous.end(), Affinity::Downstream))
        }
    }
}
