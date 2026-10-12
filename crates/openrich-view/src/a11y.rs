//! The accessibility tree (AccessKit) of the editor.
//!
//! The editor is a multiline text input whose text runs are the document's
//! visual lines, with character geometry for blocks near the viewport and
//! plain text for the rest. Each top-level block's subtree is cached by
//! node identity, so an update after typing only carries the edited block
//! and the few nodes whose place changed. Platform adapters (AccessKit on
//! native, ARIA on the web) consume the same tree.

use std::collections::HashMap;
use std::rc::Rc;

use accesskit::{
    Action, ActionData, ActionRequest, Affine, Node as AxNode, NodeId, Rect as AxRect, Role,
    TextPosition, TextSelection, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use openrich_layout::{Affinity, DocumentLayout, TextBox, TextLayout};
use openrich_model::Node;
use openrich_text::TextIndexMap;

use crate::EditorView;

/// The editor's node: a multiline text input holding the document.
pub const EDITOR_NODE: NodeId = NodeId(1);
/// Carries the scroll offset, so scrolling updates one node.
const CONTENT_NODE: NodeId = NodeId(2);
/// Bits for node ids inside one top-level block's subtree.
const LOCAL_BITS: u32 = 24;

/// One text run: the document offsets of its character boundaries,
/// relative to the top-level block (one more entry than characters).
#[derive(Debug, Clone)]
struct Run {
    id: NodeId,
    offsets: Vec<usize>,
    /// The run ends its textblock (its last character is the added `\n`).
    last: bool,
}

/// What a subtree was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SubtreeKey {
    identity: usize,
    /// The first paragraph layout when built with geometry.
    detail: Option<usize>,
}

#[derive(Debug)]
struct Subtree {
    key: SubtreeKey,
    /// The block's own node, without its placement.
    top: AxNode,
    /// Descendant nodes, sent once.
    nodes: Vec<(NodeId, AxNode)>,
    runs: Vec<Run>,
    /// Task checkboxes and their item's position relative to the block.
    checkboxes: Vec<(NodeId, usize)>,
    sent: bool,
}

/// Accessibility state kept between updates.
#[derive(Debug, Default)]
pub(crate) struct A11y {
    /// Subtree id base per (node identity, occurrence in the document).
    bases: HashMap<(usize, u32), u64>,
    next_base: u64,
    subtrees: HashMap<u64, Subtree>,
    /// Subtree base of each top-level block, as last sent.
    order: Vec<u64>,
    /// Nodes outside subtrees as last sent.
    sent: HashMap<NodeId, AxNode>,
    sent_focus: Option<NodeId>,
}

fn ax_rect(x: f32, y: f32, width: f32, height: f32) -> AxRect {
    AxRect::new(
        f64::from(x),
        f64::from(y),
        f64::from(x + width),
        f64::from(y + height),
    )
}

fn role_of(node: &Node) -> Role {
    match node.type_name() {
        "heading" => Role::Heading,
        "blockquote" => Role::Blockquote,
        "codeBlock" => Role::Code,
        "bulletList" | "orderedList" | "taskList" => Role::List,
        "listItem" | "taskItem" => Role::ListItem,
        "horizontalRule" => Role::Splitter,
        "image" => Role::Image,
        _ if node.is_textblock() => Role::Paragraph,
        _ => Role::GenericContainer,
    }
}

/// UTF-8 byte offset of each UTF-16 offset in `offsets` (sorted).
fn utf8_offsets(text: &str, offsets: &[usize]) -> Vec<usize> {
    let mut out = Vec::with_capacity(offsets.len());
    let (mut utf16, mut chars) = (0, text.char_indices().peekable());
    for &target in offsets {
        while utf16 < target {
            match chars.next() {
                Some((_, c)) => utf16 += c.len_utf16(),
                None => break,
            }
        }
        out.push(chars.peek().map_or(text.len(), |&(byte, _)| byte));
    }
    out
}

/// Builds the nodes of one top-level block.
struct SubtreeBuilder<'a> {
    base: u64,
    next: u64,
    /// Paragraph layouts of this block when it is laid out.
    texts: &'a [TextBox],
    /// Document position and y of the block (to make geometry relative).
    origin: (usize, f32),
    nodes: Vec<(NodeId, AxNode)>,
    runs: Vec<Run>,
    checkboxes: Vec<(NodeId, usize)>,
}

impl SubtreeBuilder<'_> {
    fn id(&mut self) -> NodeId {
        let id = NodeId(self.base << LOCAL_BITS | self.next);
        self.next += 1;
        id
    }

    /// Fills `ax` for `node` at `pos` and returns it (children are pushed).
    fn fill(&mut self, node: &Node, pos: usize, ax: &mut AxNode) {
        if node.type_name() == "heading" {
            let level = node.attr("level").and_then(|v| v.as_u64()).unwrap_or(1);
            ax.set_level(level as usize);
        }
        if node.type_name() == "image" {
            if let Some(alt) = node.attr("alt").and_then(|v| v.as_str()) {
                ax.set_label(alt);
            }
            return;
        }
        if node.is_textblock() {
            let text = self.texts.iter().find(|t| t.node_pos == pos).cloned();
            let runs = match text {
                Some(text) => self.detailed_runs(&text),
                None => self.plain_run(node, pos),
            };
            ax.set_children(runs);
            return;
        }
        let mut children = Vec::new();
        if node.type_name() == "taskItem" {
            let id = self.id();
            let mut checkbox = AxNode::new(Role::CheckBox);
            let checked = node
                .attr("checked")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            checkbox.set_toggled(Toggled::from(checked));
            checkbox.add_action(Action::Click);
            self.nodes.push((id, checkbox));
            self.checkboxes.push((id, pos - self.origin.0));
            children.push(id);
        }
        let mut child_pos = pos + 1;
        for child in node.children() {
            let id = self.id();
            let mut ax = AxNode::new(role_of(child));
            self.fill(child, child_pos, &mut ax);
            self.nodes.push((id, ax));
            children.push(id);
            child_pos += child.node_size();
        }
        ax.set_children(children);
    }

    /// One run per visual line, with character geometry.
    fn detailed_runs(&mut self, text: &TextBox) -> Vec<NodeId> {
        let layout: &TextLayout = &text.layout;
        let boundaries = layout.index().boundaries();
        let lines = layout.lines();
        let (top_pos, top_y) = self.origin;
        let mut ids = Vec::with_capacity(lines.len());
        for (index, line) in lines.iter().enumerate() {
            let last = index + 1 == lines.len();
            let end = if last {
                layout.len()
            } else {
                line.end_with_break
            };
            let mut offsets: Vec<usize> = boundaries
                .iter()
                .copied()
                .filter(|&b| b >= line.start && b <= end)
                .collect();
            if offsets.first() != Some(&line.start) {
                offsets.insert(0, line.start);
            }
            let bytes = utf8_offsets(layout.text(), &offsets);
            let mut value = layout.text()[bytes[0]..*bytes.last().unwrap_or(&bytes[0])].to_owned();
            let mut lengths: Vec<u8> = bytes
                .windows(2)
                .map(|w| u8::try_from(w[1] - w[0]).unwrap_or(u8::MAX))
                .collect();
            let x0 = line.left;
            let caret_x = |offset: usize| layout.caret(offset, Affinity::Downstream).x - x0;
            let mut positions: Vec<f32> = offsets[..offsets.len() - 1]
                .iter()
                .map(|&o| caret_x(o))
                .collect();
            let mut widths: Vec<f32> = offsets
                .windows(2)
                .map(|w| (caret_x(w[1]) - caret_x(w[0])).abs())
                .collect();
            if last {
                // The end of the block reads as a line break.
                value.push('\n');
                lengths.push(1);
                positions.push(line.width);
                widths.push(0.0);
                offsets.push(end);
            }
            let words = word_starts(&value, &lengths);
            let id = self.id();
            let mut run = AxNode::new(Role::TextRun);
            run.set_bounds(ax_rect(
                text.rect.x + line.left,
                text.rect.y - top_y + line.top,
                line.width.max(1.0),
                line.height,
            ));
            run.set_value(value);
            run.set_character_lengths(lengths);
            run.set_character_positions(positions);
            run.set_character_widths(widths);
            run.set_word_starts(words);
            self.nodes.push((id, run));
            self.runs.push(Run {
                id,
                offsets: offsets.iter().map(|o| text.pos - top_pos + o).collect(),
                last,
            });
            ids.push(id);
        }
        ids
    }

    /// One run with the whole text, for blocks away from the viewport.
    fn plain_run(&mut self, node: &Node, pos: usize) -> Vec<NodeId> {
        let text = openrich_text::textblock_text(node);
        let index = TextIndexMap::from_text(&text);
        let mut offsets = index.boundaries().to_vec();
        if offsets.is_empty() {
            offsets.push(0);
        }
        let bytes = utf8_offsets(&text, &offsets);
        let mut lengths: Vec<u8> = bytes
            .windows(2)
            .map(|w| u8::try_from(w[1] - w[0]).unwrap_or(u8::MAX))
            .collect();
        let mut value = text.clone();
        value.push('\n');
        lengths.push(1);
        offsets.push(*offsets.last().unwrap_or(&0));
        let words = word_starts(&value, &lengths);
        let id = self.id();
        let mut run = AxNode::new(Role::TextRun);
        run.set_value(value);
        run.set_character_lengths(lengths);
        run.set_word_starts(words);
        self.nodes.push((id, run));
        let start = pos + 1 - self.origin.0;
        self.runs.push(Run {
            id,
            offsets: offsets.iter().map(|o| start + o).collect(),
            last: true,
        });
        vec![id]
    }
}

/// Characters starting a word: non-space after a space.
fn word_starts(value: &str, lengths: &[u8]) -> Vec<u8> {
    let mut starts = Vec::new();
    let mut byte = 0;
    let mut previous_space = true;
    for (index, &len) in lengths.iter().enumerate() {
        let character = &value[byte..byte + usize::from(len)];
        let space = character.chars().all(char::is_whitespace);
        // Indices are bytes in AccessKit; very long unwrapped runs (plain
        // text far from the viewport) keep only their first words.
        if !space
            && previous_space
            && index > 0
            && let Ok(index) = u8::try_from(index)
        {
            starts.push(index);
        }
        previous_space = space;
        byte += usize::from(len);
    }
    starts
}

impl EditorView {
    /// The complete accessibility tree, for when an assistive technology
    /// connects.
    pub fn accessibility_tree(&mut self) -> TreeUpdate {
        self.a11y.sent.clear();
        self.a11y.sent_focus = None;
        for subtree in self.a11y.subtrees.values_mut() {
            subtree.sent = false;
        }
        self.accessibility_update()
            .expect("a full tree always has nodes")
    }

    /// Nodes changed since the last tree or update, or `None` when nothing
    /// changed. Hosts call this after input while an assistive technology
    /// is active.
    pub fn accessibility_update(&mut self) -> Option<TreeUpdate> {
        let layout = self.layout();
        let doc = self.editor.doc().clone();
        let mut a11y = std::mem::take(&mut self.a11y);
        let mut nodes: Vec<(NodeId, AxNode)> = Vec::new();
        let mut occurrences: HashMap<usize, u32> = HashMap::new();
        let mut used_bases = Vec::with_capacity(layout.tops.len());
        let mut top_ids = Vec::with_capacity(layout.tops.len());
        a11y.order.clear();

        for (index, child) in doc.children().enumerate() {
            let Some(top) = layout.tops.get(index) else {
                break;
            };
            let identity = child.identity();
            let occurrence = occurrences.entry(identity).or_default();
            let slot = (identity, *occurrence);
            *occurrence += 1;
            let base = *a11y.bases.entry(slot).or_insert_with(|| {
                a11y.next_base += 1;
                a11y.next_base
            });
            used_bases.push(slot);
            a11y.order.push(base);
            let texts = block_texts(&layout, top.pos, top.end());
            let key = SubtreeKey {
                identity,
                detail: texts.first().map(|t| Rc::as_ptr(&t.layout) as usize),
            };
            if a11y.subtrees.get(&base).is_none_or(|s| s.key != key) {
                let mut builder = SubtreeBuilder {
                    base,
                    next: 1,
                    texts,
                    origin: (top.pos, top.y),
                    nodes: Vec::new(),
                    runs: Vec::new(),
                    checkboxes: Vec::new(),
                };
                let mut ax = AxNode::new(role_of(child));
                builder.fill(child, top.pos, &mut ax);
                a11y.subtrees.insert(
                    base,
                    Subtree {
                        key,
                        top: ax,
                        nodes: builder.nodes,
                        runs: builder.runs,
                        checkboxes: builder.checkboxes,
                        sent: false,
                    },
                );
            }
            let subtree = a11y.subtrees.get_mut(&base).expect("just built");
            if !subtree.sent {
                nodes.extend(subtree.nodes.iter().cloned());
                subtree.sent = true;
            }
            let id = NodeId(base << LOCAL_BITS);
            let mut ax = subtree.top.clone();
            ax.set_transform(Affine::translate((0.0, f64::from(top.y))));
            ax.set_bounds(ax_rect(0.0, 0.0, layout.width, top.height));
            push_changed(&mut a11y.sent, &mut nodes, id, ax);
            top_ids.push(id);
        }
        // Forget blocks that left the document.
        let live: std::collections::HashSet<(usize, u32)> = used_bases.iter().copied().collect();
        a11y.bases.retain(|slot, _| live.contains(slot));
        let live_bases: std::collections::HashSet<u64> = a11y.bases.values().copied().collect();
        a11y.subtrees.retain(|base, _| live_bases.contains(base));
        a11y.sent
            .retain(|id, _| id.0 >> LOCAL_BITS == 0 || live_bases.contains(&(id.0 >> LOCAL_BITS)));

        let mut content = AxNode::new(Role::GenericContainer);
        content.set_transform(Affine::translate((
            f64::from(self.padding.left),
            f64::from(self.padding.top - self.scroll_y),
        )));
        content.set_bounds(ax_rect(0.0, 0.0, layout.width, layout.height));
        content.set_children(top_ids);
        push_changed(&mut a11y.sent, &mut nodes, CONTENT_NODE, content);

        let mut editor = AxNode::new(Role::MultilineTextInput);
        editor.set_transform(Affine::scale(f64::from(self.scale)));
        editor.set_bounds(ax_rect(0.0, 0.0, self.width, self.height));
        editor.set_children(vec![CONTENT_NODE]);
        editor.add_action(Action::Focus);
        editor.add_action(Action::SetTextSelection);
        editor.add_action(Action::ReplaceSelectedText);
        if !self.editor.is_editable() {
            editor.set_read_only();
        }
        if let Some(selection) = self.text_selection_for(&a11y, &layout) {
            editor.set_text_selection(selection);
        }
        push_changed(&mut a11y.sent, &mut nodes, EDITOR_NODE, editor);

        let focus = EDITOR_NODE;
        let changed = !nodes.is_empty() || a11y.sent_focus != Some(focus);
        let first = a11y.sent_focus.is_none();
        a11y.sent_focus = Some(focus);
        self.a11y = a11y;
        changed.then(|| TreeUpdate {
            nodes,
            tree: first.then(|| TreeInfo::new(EDITOR_NODE)),
            tree_id: TreeId::ROOT,
            focus,
        })
    }

    /// The selection in text run coordinates.
    fn text_selection_for(&self, a11y: &A11y, layout: &DocumentLayout) -> Option<TextSelection> {
        let selection = self.editor.selection();
        let anchor = text_position(a11y, layout, selection.anchor())?;
        let focus = text_position(a11y, layout, selection.head())?;
        Some(TextSelection { anchor, focus })
    }

    /// Performs an action requested by an assistive technology.
    pub fn accessibility_action(&mut self, request: &ActionRequest) -> bool {
        match (request.action, &request.data) {
            (Action::Focus, _) => {
                self.set_focused(true);
                true
            }
            (Action::SetTextSelection, Some(ActionData::SetTextSelection(selection))) => {
                let (Some(anchor), Some(head)) = (
                    self.doc_position(selection.anchor),
                    self.doc_position(selection.focus),
                ) else {
                    return false;
                };
                self.select_text(anchor, head, Affinity::Downstream)
            }
            (Action::ReplaceSelectedText, Some(ActionData::Value(text))) => self.insert_text(text),
            (Action::Click, _) => {
                // Task checkboxes are the only clickable nodes.
                let base = request.target_node.0 >> LOCAL_BITS;
                let (Some(top), Some(subtree)) = (self.locate(base), self.a11y.subtrees.get(&base))
                else {
                    return false;
                };
                let Some(&(_, item)) = subtree
                    .checkboxes
                    .iter()
                    .find(|(id, _)| *id == request.target_node)
                else {
                    return false;
                };
                let toggle = openrich_extensions::starter_kit::toggle_task_item(top + item);
                self.run_command(&toggle)
            }
            (Action::ScrollIntoView, _) => {
                self.scroll_into_view();
                true
            }
            _ => false,
        }
    }

    /// Where the block with subtree `base` is in the current document (it
    /// may have moved since the last update).
    fn locate(&self, base: u64) -> Option<usize> {
        let mut occurrences: HashMap<usize, u32> = HashMap::new();
        let mut pos = 0;
        for child in self.editor.doc().children() {
            let occurrence = occurrences.entry(child.identity()).or_default();
            if self.a11y.bases.get(&(child.identity(), *occurrence)) == Some(&base) {
                return Some(pos);
            }
            *occurrence += 1;
            pos += child.node_size();
        }
        None
    }

    /// The document position of a text run position from the last update.
    fn doc_position(&self, position: TextPosition) -> Option<usize> {
        let base = position.node.0 >> LOCAL_BITS;
        let subtree = self.a11y.subtrees.get(&base)?;
        let top = self.locate(base)?;
        let run = subtree.runs.iter().find(|run| run.id == position.node)?;
        let index = position.character_index.min(run.offsets.len() - 1);
        Some(top + run.offsets[index])
    }
}

/// The paragraph layouts inside `from..to`.
fn block_texts(layout: &DocumentLayout, from: usize, to: usize) -> &[TextBox] {
    let start = layout.texts.partition_point(|t| t.node_pos < from);
    let end = layout.texts.partition_point(|t| t.node_pos < to);
    &layout.texts[start..end]
}

/// Sends `node` unless it equals what was last sent under `id`.
fn push_changed(
    sent: &mut HashMap<NodeId, AxNode>,
    nodes: &mut Vec<(NodeId, AxNode)>,
    id: NodeId,
    node: AxNode,
) {
    if sent.get(&id) != Some(&node) {
        sent.insert(id, node.clone());
        nodes.push((id, node));
    }
}

/// The text run position of document position `pos`.
fn text_position(a11y: &A11y, layout: &DocumentLayout, pos: usize) -> Option<TextPosition> {
    let index = layout.top_at_pos(pos)?;
    let top = layout.tops.get(index)?;
    let base = *a11y.order.get(index)?;
    let subtree = a11y.subtrees.get(&base)?;
    let relative = pos - top.pos;
    // The run whose range holds the position; at a soft wrap the next
    // line's start wins, at a block end the line break character.
    let mut best: Option<TextPosition> = None;
    for run in &subtree.runs {
        let (first, last) = (run.offsets[0], *run.offsets.last()?);
        if relative < first || relative > last {
            continue;
        }
        let chars = run.offsets.len() - 1;
        let mut character_index = run.offsets.partition_point(|&o| o < relative).min(chars);
        if run.last && character_index == chars {
            character_index = chars.saturating_sub(1);
        }
        let position = TextPosition {
            node: run.id,
            character_index,
        };
        if relative < last || run.last {
            return Some(position);
        }
        best.get_or_insert(position);
    }
    best.or_else(|| {
        // Positions between blocks (node selections) map to the nearest run.
        let run = subtree.runs.first()?;
        Some(TextPosition {
            node: run.id,
            character_index: 0,
        })
    })
}
