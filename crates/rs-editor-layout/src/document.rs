//! Lays out a document: block structure, list markers, and position ↔
//! geometry queries across blocks.
//!
//! Layout is lazy per top-level block. A measured block keeps a
//! [`Fragment`] of boxes relative to its own origin, reused while the
//! block's node is unchanged; blocks far from the viewport only carry an
//! estimated height until they come close. Editing one block therefore
//! shapes only that block, and opening a long document shapes only what
//! is visible.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::rc::Rc;

use rs_editor_model::Node;
use rs_editor_text::Affinity;
use rs_editor_theme::Theme;
use skia_safe::textlayout::{
    FontCollection, ParagraphBuilder, ParagraphStyle, PlaceholderAlignment, PlaceholderStyle,
    TextAlign, TextBaseline, TypefaceFontProvider,
};

use crate::emoji::{self, SharedEmoji};
use crate::fonts::{font_collection, register_font};
use crate::geometry::Rect;
use crate::images::Images;
use crate::style::{BlockStyle, base_text_style, block_style, run_style, strut};
use crate::text::{CaretStop, Placeholders, Squeeze, TextLayout};
use crate::tops::{CHUNK, Chunk, Slot, Tops};

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

    fn shifted(&self, dp: usize, dy: f32) -> Self {
        Self {
            node_pos: self.node_pos + dp,
            pos: self.pos + dp,
            node: self.node.clone(),
            rect: self.rect.translate(0.0, dy),
            layout: self.layout.clone(),
        }
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
        /// The decoded image, once the host supplied it.
        image: Option<skia_safe::Image>,
        /// The host could not load it.
        failed: bool,
    },
}

impl Decoration {
    fn shifted(&self, dp: usize, dy: f32) -> Self {
        let mut out = self.clone();
        match &mut out {
            Decoration::Bullet { cy, .. } => *cy += dy,
            Decoration::Number { y, .. } => *y += dy,
            Decoration::Checkbox { rect, pos, .. } => {
                rect.y += dy;
                *pos += dp;
            }
            Decoration::QuoteBar { rect }
            | Decoration::CodeBackground { rect }
            | Decoration::Rule { rect }
            | Decoration::Image { rect, .. } => rect.y += dy,
        }
        out
    }
}

/// Every block node with its rectangle (for node selection and handles).
#[derive(Debug, Clone)]
pub struct BlockBox {
    pub pos: usize,
    pub node: Node,
    pub rect: Rect,
    pub depth: usize,
}

impl BlockBox {
    fn shifted(&self, dp: usize, dy: f32) -> Self {
        Self {
            pos: self.pos + dp,
            node: self.node.clone(),
            rect: self.rect.translate(0.0, dy),
            depth: self.depth,
        }
    }
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

/// A document laid out at one width.
///
/// `texts`, `decorations` and `blocks` hold the boxes of the measured
/// window only (in document order and coordinates); `tops` places every
/// top-level block.
#[derive(Debug, Clone)]
pub struct DocumentLayout {
    pub width: f32,
    /// Total height, partly estimated when not every block is measured.
    pub height: f32,
    pub texts: Vec<TextBox>,
    pub decorations: Vec<Decoration>,
    pub blocks: Vec<BlockBox>,
    pub tops: Tops,
    /// The vertical range (document coordinates) whose blocks are all in
    /// `texts`/`decorations`/`blocks`.
    pub covered: (f32, f32),
    /// Indices into `tops` of the blocks whose boxes are present.
    shown: Vec<usize>,
}

type TextKey = (usize, u32, bool);

#[derive(Debug, Clone)]
struct CacheEntry {
    node: Node,
    layout: Rc<TextLayout>,
    /// Layout pass that last used this entry.
    used: u64,
}

/// The boxes of one top-level block, relative to position 0 and y 0.
#[derive(Debug, Default)]
struct Fragment {
    height: f32,
    texts: Vec<TextBox>,
    decorations: Vec<Decoration>,
    blocks: Vec<BlockBox>,
    keys: Vec<TextKey>,
}

#[derive(Debug, Clone)]
struct Entry {
    node: Node,
    heading: bool,
    leaf: bool,
    height: f32,
    fragment: Option<Rc<Fragment>>,
}

/// Builds paragraphs with the theme's fonts and styles.
struct Shaper {
    fonts: FontCollection,
    provider: TypefaceFontProvider,
    /// Families added at run time, tried after the bundled ones.
    fallbacks: Vec<String>,
    /// Characters no font could show, since the last
    /// [`LayoutEngine::take_missing_characters`].
    missing: RefCell<BTreeSet<u32>>,
    /// Emoji images from the host, when emoji are drawn that way.
    emoji: Option<SharedEmoji>,
    /// Pictures of image nodes, decoded by the host.
    images: RefCell<Images>,
    theme: Theme,
}

/// The style of UI text ([`LayoutEngine::ui_label`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiText {
    pub size: f32,
    pub line_height: f32,
    pub weight: u16,
    pub color: rs_editor_theme::Color,
    pub mono: bool,
}

impl UiText {
    /// shadcn's `text-sm`: 14 px on a 20 px line.
    pub fn small(color: rs_editor_theme::Color) -> Self {
        Self {
            size: 14.0,
            line_height: 20.0,
            weight: 400,
            color,
            mono: false,
        }
    }

    /// shadcn's `text-xs`: 12 px on a 16 px line.
    pub fn extra_small(color: rs_editor_theme::Color) -> Self {
        Self {
            size: 12.0,
            line_height: 16.0,
            weight: 400,
            color,
            mono: false,
        }
    }
}

/// Lays out documents, reusing unchanged blocks.
pub struct LayoutEngine {
    shaper: Shaper,
    texts: HashMap<TextKey, CacheEntry>,
    generation: u64,
    entries: Vec<Entry>,
    width: f32,
    chunks: Vec<Chunk>,
    /// Chunks to rebuild from their entries.
    dirty: Vec<usize>,
    /// Rebuild every chunk from this one on (block count changed).
    rebuild_from: Option<usize>,
}

impl fmt::Debug for LayoutEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LayoutEngine")
            .field("blocks", &self.entries.len())
            .field("measured", &self.cached_blocks())
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
    shaper: &'a Shaper,
    cache: &'a mut HashMap<TextKey, CacheEntry>,
    generation: u64,
    out: Fragment,
}

/// Blocks measured beyond the edges of the view's window when the caller
/// asks for a position, so that moving across blocks finds its neighbor.
const NEIGHBOR_LIMIT: usize = 16;

impl LayoutEngine {
    /// `system_fallback` lets the platform's fonts cover emoji and scripts
    /// missing from the bundle; turn it off for deterministic tests.
    pub fn new(theme: Theme, system_fallback: bool) -> Self {
        let (fonts, provider) = font_collection(system_fallback);
        Self {
            shaper: Shaper {
                fonts,
                provider,
                fallbacks: Vec::new(),
                missing: RefCell::new(BTreeSet::new()),
                emoji: None,
                images: RefCell::new(Images::default()),
                theme,
            },
            texts: HashMap::new(),
            generation: 0,
            entries: Vec::new(),
            width: 0.0,
            chunks: Vec::new(),
            dirty: Vec::new(),
            rebuild_from: None,
        }
    }

    fn reset(&mut self) {
        self.entries.clear();
        self.chunks.clear();
        self.dirty.clear();
        self.rebuild_from = Some(0);
    }

    pub fn theme(&self) -> &Theme {
        &self.shaper.theme
    }

    /// Changes the theme; every block is laid out again.
    pub fn set_theme(&mut self, theme: Theme) {
        if theme != self.shaper.theme {
            self.shaper.theme = theme;
            self.texts.clear();
            self.reset();
        }
    }

    /// Adds a font (file bytes) as a fallback for characters the bundled
    /// fonts lack, under `family` or the font's own family name. Every block
    /// is laid out again. Returns the family, or `None` for bytes that are
    /// not a font.
    pub fn add_font(&mut self, bytes: &[u8], family: Option<&str>) -> Option<String> {
        let family = register_font(&mut self.shaper.provider, bytes, family)?;
        if !self.shaper.fallbacks.contains(&family) {
            self.shaper.fallbacks.push(family.clone());
        }
        self.shaper.fonts.clear_caches();
        self.texts.clear();
        self.reset();
        Some(family)
    }

    /// Draws emoji with images from the host (see
    /// [`LayoutEngine::add_emoji_image`]) rather than with a font: for
    /// platforms without emoji fonts, such as the web. Every block is laid
    /// out again.
    pub fn set_emoji_images(&mut self, on: bool) {
        if on == self.shaper.emoji.is_some() {
            return;
        }
        self.shaper.emoji = on.then(SharedEmoji::default);
        self.texts.clear();
        self.reset();
    }

    /// The image (any size, drawn into a square) for emoji `grapheme`.
    /// Only painting changes, so no layout is needed.
    pub fn add_emoji_image(&mut self, grapheme: &str, image: skia_safe::Image) {
        if let Some(emoji) = &self.shaper.emoji {
            let mut emoji = emoji.borrow_mut();
            emoji.missing.remove(grapheme);
            emoji.images.insert(grapheme.to_owned(), image);
        }
    }

    /// Emoji laid out since the last call that have no image yet.
    pub fn take_missing_emoji(&mut self) -> Vec<String> {
        self.shaper.emoji.as_ref().map_or_else(Vec::new, |emoji| {
            std::mem::take(&mut emoji.borrow_mut().missing)
                .into_iter()
                .collect()
        })
    }

    /// The picture for image nodes with `src`, decoded by the host; `scale`
    /// is its pixels per CSS pixel (2 for a 2× image), which sets the size
    /// it shows at without `width` and `height` attributes. Blocks are laid
    /// out again, since images without those attributes take its size.
    pub fn add_image(&mut self, src: &str, image: skia_safe::Image, scale: f32) {
        self.shaper.images.borrow_mut().add(src, image, scale);
        self.forget_image_blocks(src);
    }

    /// Lays out again only the top-level blocks showing `src` (the rest
    /// of a long document keeps its measurements).
    fn forget_image_blocks(&mut self, src: &str) {
        let shows = |node: &Node| {
            node.type_name() == "image" && node.attr("src").and_then(|v| v.as_str()) == Some(src)
        };
        for index in 0..self.entries.len() {
            let node = &self.entries[index].node;
            let mut found = shows(node);
            if !found {
                node.descendants(&mut |child, _, _, _| {
                    found |= shows(child);
                    !found
                });
            }
            if found && self.entries[index].fragment.take().is_some() {
                self.dirty.push(index / CHUNK);
            }
        }
    }

    /// Releases the pictures of image sources not in `live`.
    pub fn retain_images(&mut self, live: &std::collections::HashSet<String>) {
        self.shaper.images.borrow_mut().retain(live);
    }

    /// The host could not load `src`: it stays a placeholder.
    pub fn image_failed(&mut self, src: &str) {
        self.shaper.images.borrow_mut().fail(src);
        self.forget_image_blocks(src);
    }

    /// Asks for a broken image again (the next layout reports it missing);
    /// false when `src` had not failed.
    pub fn retry_image(&mut self, src: &str) -> bool {
        let failed = self.shaper.images.borrow_mut().retry(src);
        if failed {
            self.forget_image_blocks(src);
        }
        failed
    }

    /// Image sources laid out since the last call that are neither loaded
    /// nor failed, for the host to load.
    pub fn take_missing_images(&mut self) -> Vec<String> {
        std::mem::take(&mut self.shaper.images.borrow_mut().missing)
            .into_iter()
            .collect()
    }

    /// Characters laid out since the last call that no font could show
    /// (Unicode scalar values), for hosts that load fonts on demand.
    pub fn take_missing_characters(&mut self) -> Vec<u32> {
        std::mem::take(&mut *self.shaper.missing.borrow_mut())
            .into_iter()
            .collect()
    }

    /// Number of measured top-level blocks.
    pub fn cached_blocks(&self) -> usize {
        self.entries.iter().filter(|e| e.fragment.is_some()).count()
    }

    /// Lays out all of `doc` at `width` logical pixels.
    pub fn layout(&mut self, doc: &Node, width: f32) -> DocumentLayout {
        self.layout_window(doc, width, None, &[])
    }

    /// Lays out the blocks of `doc` that intersect `window` (document
    /// coordinates; `None` for everything) and the blocks around each of
    /// `include` (positions such as the selection ends). Other blocks keep
    /// their last measured or estimated height.
    pub fn layout_window(
        &mut self,
        doc: &Node,
        width: f32,
        window: Option<(f32, f32)>,
        include: &[usize],
    ) -> DocumentLayout {
        self.generation += 1;
        self.sync(doc, width);
        let mut tops = self.refresh();

        let mut forced = Vec::new();
        for &pos in include {
            if let Some(index) = tops.at_pos(pos) {
                forced.push(index);
                for step in [-1isize, 1] {
                    let mut i = index as isize;
                    for _ in 0..NEIGHBOR_LIMIT {
                        i += step;
                        let Some(entry) = usize::try_from(i).ok().and_then(|i| self.entries.get(i))
                        else {
                            break;
                        };
                        forced.push(i as usize);
                        if !entry.leaf {
                            break;
                        }
                    }
                }
            }
        }
        forced.sort_unstable();
        forced.dedup();

        // Measuring changes heights, which moves the window's blocks: repeat
        // until every block in the window is measured.
        loop {
            let range = window_range(&tops, window);
            let mut measured_any = false;
            for index in range.chain(forced.iter().copied()) {
                if self.entries[index].fragment.is_none() {
                    self.measure(index, width);
                    measured_any = true;
                }
            }
            if !measured_any {
                break;
            }
            tops = self.refresh();
        }

        let mut shown: Vec<usize> = window_range(&tops, window).chain(forced).collect();
        shown.sort_unstable();
        shown.dedup();
        let height = tops.height();
        let mut out = DocumentLayout {
            width,
            height,
            texts: Vec::new(),
            decorations: Vec::new(),
            blocks: Vec::new(),
            tops: tops.clone(),
            covered: window.unwrap_or((f32::NEG_INFINITY, f32::INFINITY)),
            shown: Vec::new(),
        };
        for &index in &shown {
            let Some(top) = tops.get(index) else {
                continue;
            };
            let Some(fragment) = &self.entries[index].fragment else {
                continue;
            };
            for key in &fragment.keys {
                if let Some(entry) = self.texts.get_mut(key) {
                    entry.used = self.generation;
                }
            }
            let (dp, dy) = (top.pos, top.y);
            out.texts
                .extend(fragment.texts.iter().map(|t| t.shifted(dp, dy)));
            out.decorations
                .extend(fragment.decorations.iter().map(|d| d.shifted(dp, dy)));
            out.blocks
                .extend(fragment.blocks.iter().map(|b| b.shifted(dp, dy)));
        }
        // Keep paragraphs used in this pass or the last one, so editing a
        // block that was just on screen reshapes only what changed.
        let generation = self.generation;
        self.texts.retain(|_, entry| entry.used + 1 >= generation);
        out.shown = shown;
        out
    }

    /// Matches the document's top-level blocks with the previous pass,
    /// keeping fragments of unchanged nodes.
    fn sync(&mut self, doc: &Node, width: f32) {
        if width != self.width {
            self.reset();
            self.width = width;
        }
        let children: Vec<&Node> = doc.children().collect();
        let old_len = self.entries.len();
        let common = old_len.min(children.len());
        let prefix = (0..common)
            .take_while(|&i| self.entries[i].node.ptr_eq(children[i]))
            .count();
        let suffix = (0..common - prefix)
            .take_while(|&i| {
                self.entries[old_len - 1 - i]
                    .node
                    .ptr_eq(children[children.len() - 1 - i])
            })
            .count();
        let old_middle = prefix..old_len - suffix;
        let new_middle = &children[prefix..children.len() - suffix];
        if old_middle.is_empty() && new_middle.is_empty() {
            return;
        }
        let entries = {
            let pool: HashMap<usize, &Entry> = self.entries[old_middle.clone()]
                .iter()
                .map(|entry| (entry.node.identity(), entry))
                .collect();
            new_middle
                .iter()
                .map(|child| match pool.get(&child.identity()) {
                    Some(entry) if entry.node.ptr_eq(child) => (*entry).clone(),
                    _ => Entry {
                        node: (*child).clone(),
                        heading: child.type_name() == "heading",
                        leaf: child.is_leaf(),
                        height: self.shaper.estimate(child, width),
                        fragment: None,
                    },
                })
                .collect::<Vec<_>>()
        };
        let resized = old_middle.len() != entries.len();
        let changed = entries.len();
        self.entries.splice(old_middle, entries);
        if resized {
            let from = prefix / CHUNK;
            self.rebuild_from = Some(self.rebuild_from.map_or(from, |current| current.min(from)));
        } else {
            // The block after the change may get a different gap.
            self.dirty
                .extend(prefix / CHUNK..=(prefix + changed) / CHUNK);
        }
    }

    fn slot(&self, index: usize) -> Slot {
        let spacing = &self.shaper.theme.spacing;
        let entry = &self.entries[index];
        let gap = match index {
            0 => 0.0,
            _ if entry.heading => spacing.block_gap + spacing.heading_gap,
            _ => spacing.block_gap,
        };
        Slot {
            size: entry.node.node_size(),
            gap,
            height: entry.height,
            measured: entry.fragment.is_some(),
        }
    }

    /// Rebuilds changed chunks and snapshots the block places.
    fn refresh(&mut self) -> Tops {
        let count = self.entries.len().div_ceil(CHUNK);
        let chunk = |engine: &Self, k: usize| {
            let end = ((k + 1) * CHUNK).min(engine.entries.len());
            Chunk::new((k * CHUNK..end).map(|i| engine.slot(i)).collect())
        };
        if let Some(from) = self.rebuild_from.take() {
            let from = from.min(self.chunks.len());
            self.chunks.truncate(from);
            for k in from..count {
                let rebuilt = chunk(self, k);
                self.chunks.push(rebuilt);
            }
        }
        self.chunks.truncate(count);
        let mut dirty = std::mem::take(&mut self.dirty);
        dirty.sort_unstable();
        dirty.dedup();
        for k in dirty {
            if k < self.chunks.len() {
                self.chunks[k] = chunk(self, k);
            }
        }
        Tops::new(&self.chunks)
    }

    fn measure(&mut self, index: usize, width: f32) {
        let node = self.entries[index].node.clone();
        let mut builder = Builder {
            shaper: &self.shaper,
            cache: &mut self.texts,
            generation: self.generation,
            out: Fragment::default(),
        };
        let ctx = Ctx {
            italic: false,
            list_depth: 0,
            depth: 0,
        };
        let height = builder.block(&node, 0, 0.0, 0.0, width, ctx);
        let mut fragment = builder.out;
        fragment.height = height;
        let entry = &mut self.entries[index];
        entry.height = height;
        entry.fragment = Some(Rc::new(fragment));
        self.dirty.push(index / CHUNK);
    }

    /// Placeholder text for the empty textblock `node`, in the block's
    /// style and the muted foreground color.
    pub fn placeholder(&self, node: &Node, text: &str, width: f32) -> TextLayout {
        self.shaper.placeholder(node, text, width)
    }

    /// One line of UI text (menus, popovers) in the theme's fonts, cut
    /// with an ellipsis past `max_width`.
    pub fn ui_label(&self, text: &str, style: &UiText, max_width: Option<f32>) -> TextLayout {
        self.shaper.ui_label(text, style, max_width)
    }
}

/// Indices of the blocks intersecting `window` (all when `None`).
fn window_range(tops: &Tops, window: Option<(f32, f32)>) -> std::ops::Range<usize> {
    match window {
        None => 0..tops.len(),
        Some((from, to)) => tops.range_y(from, to),
    }
}

impl Shaper {
    /// A cheap height guess for a block that is not measured yet.
    fn estimate(&self, node: &Node, width: f32) -> f32 {
        let theme = &self.theme;
        let spacing = &theme.spacing;
        if node.is_textblock() {
            let style = block_style(theme, node, false);
            let pad = if node.node_type().spec().code {
                spacing.code_block_padding * 2.0
            } else {
                0.0
            };
            let advance = style.size * if style.mono { 0.6 } else { 0.5 };
            let per_line = ((width - pad) / advance).max(1.0);
            let lines = (node.content().size() as f32 / per_line).ceil().max(1.0);
            return lines * style.line_height + pad;
        }
        if node.is_leaf() {
            return leaf_height(node, width, &self.images.borrow());
        }
        let list = is_list(node);
        let indent = if list {
            spacing.list_indent
        } else if node.type_name() == "blockquote" {
            spacing.quote_indent
        } else {
            0.0
        };
        let in_item =
            node.node_type().is_in_group("listItem") || node.type_name().ends_with("Item");
        let gap = if list || in_item {
            spacing.list_item_gap
        } else {
            spacing.block_gap
        };
        let mut height = 0.0;
        for (index, child) in node.children().enumerate() {
            if index > 0 {
                height += gap;
            }
            height += self.estimate(child, (width - indent).max(1.0));
        }
        height
    }

    fn text_layout(&self, node: &Node, width: f32, italic: bool) -> TextLayout {
        let theme = &self.theme;
        let extra = &self.fallbacks;
        let block: BlockStyle = block_style(theme, node, italic);
        let base = base_text_style(theme, &block, extra);
        let mut paragraph_style = ParagraphStyle::new();
        paragraph_style.set_text_style(&base);
        paragraph_style.set_strut_style(strut(theme, &block, extra));
        paragraph_style.set_text_align(block.align);
        paragraph_style.set_apply_rounding_hack(false);
        paragraph_style.turn_hinting_off();
        let mut builder = ParagraphBuilder::new(&paragraph_style, self.fonts.clone());
        let mut text = String::new();
        // Content and paragraph lengths differ by the emoji drawn as images.
        let (mut doc_units, mut para_units) = (0usize, 0usize);
        let mut squeezes: Vec<Squeeze> = Vec::new();
        let mut placeholders: Placeholders = Vec::new();
        for child in node.children() {
            let style = run_style(theme, &block, child.marks(), extra);
            if let Some(run) = child.text() {
                let pieces = match &self.emoji {
                    Some(_) => emoji::segments(run),
                    None => vec![(run, false)],
                };
                for (piece, is_emoji) in pieces {
                    let units = piece.encode_utf16().count();
                    builder.push_style(&style);
                    if is_emoji && let Some(emoji) = &self.emoji {
                        let size = style.font_size() * 1.2;
                        builder.add_placeholder(&PlaceholderStyle::new(
                            size,
                            size,
                            PlaceholderAlignment::Baseline,
                            TextBaseline::Alphabetic,
                            size * 0.8,
                        ));
                        squeezes.push(Squeeze {
                            doc: doc_units,
                            len: units,
                            para: para_units,
                        });
                        placeholders.push(Some(piece.to_owned()));
                        let mut emoji = emoji.borrow_mut();
                        if !emoji.images.contains_key(piece) {
                            emoji.missing.insert(piece.to_owned());
                        }
                        para_units += 1;
                    } else {
                        builder.add_text(piece);
                        para_units += units;
                    }
                    builder.pop();
                    doc_units += units;
                }
                text.push_str(run);
            } else if child.node_type().spec().leaf_text.as_deref() == Some("\n") {
                builder.push_style(&style);
                builder.add_text("\n");
                builder.pop();
                text.push('\n');
                doc_units += 1;
                para_units += 1;
            } else {
                placeholders.push(None);
                doc_units += 1;
                para_units += 1;
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
        if paragraph.unresolved_glyphs().is_some_and(|count| count > 0) {
            let mut missing = self.missing.borrow_mut();
            for c in paragraph.unresolved_codepoints() {
                if let Ok(c) = u32::try_from(c)
                    && c > 0x20
                    && c != 0xfffc
                {
                    missing.insert(c);
                }
            }
        }
        TextLayout::with_emoji(
            paragraph,
            text,
            width,
            block.align,
            block.line_height,
            block.size * 1.3,
            squeezes,
            placeholders,
            self.emoji.clone(),
        )
    }

    fn placeholder(&self, node: &Node, text: &str, width: f32) -> TextLayout {
        let theme = &self.theme;
        let mut block: BlockStyle = block_style(theme, node, false);
        block.color = theme.palette.muted_foreground;
        let style = base_text_style(theme, &block, &self.fallbacks);
        let mut paragraph_style = ParagraphStyle::new();
        paragraph_style.set_text_style(&style);
        paragraph_style.set_strut_style(strut(theme, &block, &self.fallbacks));
        paragraph_style.set_text_align(block.align);
        paragraph_style.turn_hinting_off();
        let mut builder = ParagraphBuilder::new(&paragraph_style, self.fonts.clone());
        builder.push_style(&style);
        builder.add_text(text);
        let mut paragraph = builder.build();
        paragraph.layout(width.max(1.0));
        TextLayout::new(
            paragraph,
            text.to_owned(),
            width,
            block.align,
            block.line_height,
            block.size * 1.3,
        )
    }

    fn ui_label(&self, text: &str, ui: &UiText, max_width: Option<f32>) -> TextLayout {
        let theme = &self.theme;
        let block = BlockStyle {
            size: ui.size,
            line_height: ui.line_height,
            weight: ui.weight,
            letter_spacing_em: 0.0,
            mono: ui.mono,
            italic: false,
            color: ui.color,
            align: TextAlign::Left,
        };
        let style = base_text_style(theme, &block, &self.fallbacks);
        let mut paragraph_style = ParagraphStyle::new();
        paragraph_style.set_text_style(&style);
        paragraph_style.set_strut_style(strut(theme, &block, &self.fallbacks));
        paragraph_style.turn_hinting_off();
        paragraph_style.set_apply_rounding_hack(false);
        if max_width.is_some() {
            paragraph_style.set_max_lines(1);
            paragraph_style.set_ellipsis("…");
        }
        let mut builder = ParagraphBuilder::new(&paragraph_style, self.fonts.clone());
        builder.push_style(&style);
        // One line: hard breaks would start new ones.
        let text = text.replace(['\n', '\r'], " ");
        builder.add_text(&text);
        let mut paragraph = builder.build();
        paragraph.layout(100_000.0);
        let natural = paragraph.max_intrinsic_width().ceil();
        let width = max_width.map_or(natural, |max| natural.min(max.max(1.0)));
        paragraph.layout(width + 1.0);
        TextLayout::new(
            paragraph,
            text,
            width,
            TextAlign::Left,
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
        let style = base_text_style(theme, &block, &self.fallbacks);
        paragraph_style.set_text_style(&style);
        paragraph_style.set_strut_style(strut(theme, &block, &self.fallbacks));
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

/// Height of a leaf block (rule, image) at `width`.
fn leaf_height(node: &Node, width: f32, images: &Images) -> f32 {
    match node.type_name() {
        "image" => image_size(node, width, images).1,
        _ => 24.0,
    }
}

/// The size an image shows at in a column `width` wide: its `width` and
/// `height` attributes, else the loaded image's own size (in CSS pixels),
/// else a 16:9 box until it loads; never wider than the column.
fn image_size(node: &Node, width: f32, images: &Images) -> (f32, f32) {
    let attr = |name: &str| node.attr(name).and_then(|v| v.as_f64()).map(|v| v as f32);
    let natural = node
        .attr("src")
        .and_then(|v| v.as_str())
        .and_then(|src| images.size(src));
    match (attr("width"), attr("height"), natural) {
        (Some(w), Some(h), _) if w > 0.0 && h > 0.0 => {
            let shown = w.min(width);
            (shown, shown * h / w)
        }
        (Some(w), _, Some((nw, nh))) if w > 0.0 => {
            let shown = w.min(width);
            (shown, shown * nh / nw)
        }
        (_, _, Some((nw, nh))) => {
            let shown = nw.min(width);
            (shown, shown * nh / nw)
        }
        _ => (width.min(640.0), width.min(640.0) * 9.0 / 16.0),
    }
}

impl Builder<'_> {
    fn theme(&self) -> &Theme {
        &self.shaper.theme
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
        let layout = match self.cache.get_mut(&key) {
            Some(entry) if entry.node.ptr_eq(node) => {
                entry.used = self.generation;
                entry.layout.clone()
            }
            _ => {
                let layout = Rc::new(self.shaper.text_layout(node, width, ctx.italic));
                self.cache.insert(
                    key,
                    CacheEntry {
                        node: node.clone(),
                        layout: layout.clone(),
                        used: self.generation,
                    },
                );
                layout
            }
        };
        self.out.keys.push(key);
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
                    let label = Rc::new(self.shaper.label(&format!("{}.", start + index as u64)));
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
                let mut images = self.shaper.images.borrow_mut();
                let (w, h) = image_size(node, width, &images);
                let text = |name: &str| node.attr(name).and_then(|v| v.as_str()).map(str::to_owned);
                let src = text("src");
                let (image, failed) = match &src {
                    Some(src) => images.lookup(src),
                    None => (None, true),
                };
                self.out.decorations.push(Decoration::Image {
                    rect: Rect::new(x, y, w, h),
                    src,
                    alt: text("alt"),
                    image,
                    failed,
                });
                h
            }
            _ => 24.0,
        }
    }
}

impl DocumentLayout {
    /// Index into `tops` of the top-level block containing `pos`.
    pub fn top_at_pos(&self, pos: usize) -> Option<usize> {
        self.tops.at_pos(pos)
    }

    /// Index into `tops` of the first block reaching below `y`.
    pub fn top_at_y(&self, y: f32) -> Option<usize> {
        self.tops.at_y(y)
    }

    /// Whether the boxes of the block containing `pos` are laid out.
    pub fn shows(&self, pos: usize) -> bool {
        self.top_at_pos(pos)
            .is_some_and(|index| self.shown.binary_search(&index).is_ok())
    }

    /// Whether every block intersecting `from..to` (document y) is laid out.
    pub fn covers(&self, from: f32, to: f32) -> bool {
        self.covered.0 <= from && to <= self.covered.1
    }

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
