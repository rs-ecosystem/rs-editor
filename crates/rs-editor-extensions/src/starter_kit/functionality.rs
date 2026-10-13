//! Extensions without schema of their own: the core keymap and commands,
//! history, alignment, typography, character count and placeholders.

use std::any::Any;
use std::sync::Arc;

use rs_editor_commands as pm;
use rs_editor_history as history;
use rs_editor_model::{AttrSpec, Node};
use rs_editor_state::{
    Command, Dispatch, EditorState, Plugin, Selection, Transaction, ViewContext, command,
};
use serde_json::{Value, json};
use unicode_segmentation::UnicodeSegmentation;

use super::util::*;
use crate::core_commands::*;
use crate::extension::{CommandFactory, Extension, ExtensionContext, GlobalAttribute};
use crate::helpers::first;
use crate::inputrules::{InputRule, text_input_rule, undo_input_rule};

type View<'a> = Option<&'a dyn ViewContext>;

/// Backspace in an empty first block that is not a paragraph turns it
/// into a paragraph (Tiptap's core Backspace handling).
fn clear_first_block(state: &EditorState, dispatch: Dispatch, view: View) -> bool {
    let selection = state.selection();
    let Some(anchor) = selection.cursor() else {
        return false;
    };
    let parent = anchor.parent();
    let parent_pos = if parent.is_textblock() && anchor.pos() > 0 {
        state.doc().resolve(anchor.pos() - 1).ok()
    } else {
        Some(anchor.clone())
    };
    let Some(parent_pos) = parent_pos else {
        return false;
    };
    let isolating = parent_pos.parent().node_type().spec().isolating;
    let block_start = anchor.pos() - anchor.parent_offset();
    let at_start = if isolating && parent_pos.parent().child_count() == 1 {
        block_start == anchor.pos()
    } else {
        Selection::at_start(state.doc()).from() == anchor.pos()
    };
    if !parent.is_textblock()
        || parent.content().size() > 0
        || !at_start
        || parent.type_name() == "paragraph"
    {
        return false;
    }
    clear_nodes(state, dispatch, view)
}

/// Default key bindings and the generic Tiptap core commands. Always
/// included by [`crate::Editor`]; its key bindings run last.
#[derive(Debug, Clone, Default)]
pub struct Core;

fn backspace() -> Command {
    first(vec![
        command(undo_input_rule),
        command(clear_first_block),
        command(pm::delete_selection),
        command(pm::join_backward),
        command(pm::select_node_backward),
    ])
}

fn delete() -> Command {
    first(vec![
        command(pm::delete_selection),
        command(pm::join_forward),
        command(pm::select_node_forward),
    ])
}

fn enter() -> Command {
    first(vec![
        command(pm::newline_in_code),
        command(pm::create_paragraph_near),
        command(pm::lift_empty_block),
        command(pm::split_block_keep_marks),
    ])
}

fn usize_arg(args: &Value, key: &str) -> Option<usize> {
    args.get(key).and_then(Value::as_u64).map(|n| n as usize)
}

fn type_name_arg(command: &str, args: &Value) -> Result<String, crate::ExtensionError> {
    args.get("type")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(command, "`type` must be a node or mark name"))
}

/// Parses `content` (a node, an array of nodes, or plain text) for insertion.
pub(crate) fn content_nodes(
    schema: &rs_editor_model::Schema,
    command: &str,
    content: &Value,
) -> Result<Vec<Node>, crate::ExtensionError> {
    let parse = |value: &Value| {
        schema
            .node_from_json(value)
            .map_err(crate::ExtensionError::Schema)
    };
    match content {
        Value::String(text) if text.is_empty() => Err(invalid(command, "content is empty")),
        Value::String(text) => Ok(vec![schema.text(text.as_str(), Default::default())?]),
        Value::Array(items) => items.iter().map(parse).collect(),
        Value::Object(_) => Ok(vec![parse(content)?]),
        _ => Err(invalid(
            command,
            "content must be text, a node or a node array",
        )),
    }
}

impl Extension for Core {
    fn name(&self) -> &str {
        "core"
    }

    fn priority(&self) -> i32 {
        -1000
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let schema = ctx.schema.clone();
        let simple =
            |name: &str, f: rs_editor_state::CommandFn| (name.to_owned(), no_args(command(f)));
        let mut commands = vec![
            simple("selectAll", pm::select_all),
            simple("deleteSelection", pm::delete_selection),
            simple("joinBackward", pm::join_backward),
            simple("joinForward", pm::join_forward),
            simple("joinTextblockBackward", pm::join_textblock_backward),
            simple("joinTextblockForward", pm::join_textblock_forward),
            simple("joinUp", pm::join_up),
            simple("joinDown", pm::join_down),
            simple("selectNodeBackward", pm::select_node_backward),
            simple("selectNodeForward", pm::select_node_forward),
            simple("selectParentNode", pm::select_parent_node),
            simple("selectTextblockStart", pm::select_textblock_start),
            simple("selectTextblockEnd", pm::select_textblock_end),
            simple("splitBlock", pm::split_block_keep_marks),
            simple("liftEmptyBlock", pm::lift_empty_block),
            simple("createParagraphNear", pm::create_paragraph_near),
            simple("newlineInCode", pm::newline_in_code),
            simple("exitCode", pm::exit_code),
            simple("lift", pm::lift),
            simple("clearNodes", clear_nodes),
            simple("unsetAllMarks", unset_all_marks),
            simple("clearContent", clear_content),
            simple("undoInputRule", undo_input_rule),
        ];
        let s = schema.clone();
        commands.push((
            "toggleMark".into(),
            with_args(move |args| {
                let name = type_name_arg("toggleMark", args)?;
                let extend = args
                    .get("extendEmptyMarkRange")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let attrs = args
                    .get("attrs")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                Ok(toggle_mark(s.mark_type(&name)?, attrs, extend))
            }),
        ));
        let s = schema.clone();
        commands.push((
            "setMark".into(),
            with_args(move |args| {
                let name = type_name_arg("setMark", args)?;
                let attrs = args
                    .get("attrs")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                Ok(set_mark(s.mark_type(&name)?, attrs))
            }),
        ));
        let s = schema.clone();
        commands.push((
            "unsetMark".into(),
            with_args(move |args| {
                let name = type_name_arg("unsetMark", args)?;
                let extend = args
                    .get("extendEmptyMarkRange")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Ok(unset_mark(s.mark_type(&name)?, extend))
            }),
        ));
        let s = schema.clone();
        commands.push((
            "extendMarkRange".into(),
            with_args(move |args| {
                let name = type_name_arg("extendMarkRange", args)?;
                Ok(extend_mark_range(
                    s.mark_type(&name)?,
                    args.get("attrs").and_then(Value::as_object).cloned(),
                ))
            }),
        ));
        let s = schema.clone();
        commands.push((
            "setNode".into(),
            with_args(move |args| {
                let name = type_name_arg("setNode", args)?;
                let attrs = args
                    .get("attrs")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                Ok(set_node(s.node_type(&name)?, attrs))
            }),
        ));
        let s = schema.clone();
        commands.push((
            "toggleNode".into(),
            with_args(move |args| {
                let name = type_name_arg("toggleNode", args)?;
                let toggle = args
                    .get("toggleType")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("toggleNode", "`toggleType` is required"))?;
                let attrs = args
                    .get("attrs")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                Ok(toggle_node(
                    s.node_type(&name)?,
                    s.node_type(toggle)?,
                    attrs,
                ))
            }),
        ));
        let s = schema.clone();
        commands.push((
            "wrapIn".into(),
            with_args(move |args| {
                let name = type_name_arg("wrapIn", args)?;
                Ok(pm::wrap_in(
                    s.node_type(&name)?,
                    args.get("attrs").and_then(Value::as_object).cloned(),
                ))
            }),
        ));
        let s = schema.clone();
        commands.push((
            "toggleWrap".into(),
            with_args(move |args| {
                let name = type_name_arg("toggleWrap", args)?;
                let attrs = args
                    .get("attrs")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                Ok(toggle_wrap(s.node_type(&name)?, attrs))
            }),
        ));
        let s = schema.clone();
        commands.push((
            "toggleList".into(),
            with_args(move |args| {
                let list = type_name_arg("toggleList", args)?;
                let item = args
                    .get("itemType")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("toggleList", "`itemType` is required"))?;
                let attrs = args
                    .get("attrs")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                Ok(toggle_list(s.node_type(&list)?, s.node_type(item)?, attrs))
            }),
        ));
        commands.push((
            "updateAttributes".into(),
            with_args(move |args| {
                let name = type_name_arg("updateAttributes", args)?;
                let attrs = args
                    .get("attrs")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                Ok(update_attributes(name, attrs))
            }),
        ));
        let s = schema.clone();
        commands.push((
            "insertContent".into(),
            with_args(move |args| {
                let content = args.get("content").unwrap_or(args);
                Ok(insert_content(content_nodes(&s, "insertContent", content)?))
            }),
        ));
        let s = schema.clone();
        commands.push((
            "insertContentAt".into(),
            with_args(move |args| {
                let from = usize_arg(args, "from")
                    .ok_or_else(|| invalid("insertContentAt", "`from` is required"))?;
                let to = usize_arg(args, "to").unwrap_or(from);
                let content = args
                    .get("content")
                    .ok_or_else(|| invalid("insertContentAt", "`content` is required"))?;
                Ok(insert_content_at(
                    from,
                    to,
                    content_nodes(&s, "insertContentAt", content)?,
                ))
            }),
        ));
        let s = schema.clone();
        commands.push((
            "setContent".into(),
            with_args(move |args| {
                let content = args.get("content").unwrap_or(args);
                Ok(set_content(s.node_from_json(content)?))
            }),
        ));
        commands.push((
            "setTextSelection".into(),
            with_args(|args| {
                let from = usize_arg(args, "from")
                    .ok_or_else(|| invalid("setTextSelection", "`from` is required"))?;
                Ok(set_text_selection(
                    from,
                    usize_arg(args, "to").unwrap_or(from),
                ))
            }),
        ));
        commands.push((
            "setNodeSelection".into(),
            with_args(|args| {
                let pos = usize_arg(args, "pos")
                    .ok_or_else(|| invalid("setNodeSelection", "`pos` is required"))?;
                Ok(set_node_selection(pos))
            }),
        ));
        commands.push((
            "deleteRange".into(),
            with_args(|args| {
                let from = usize_arg(args, "from")
                    .ok_or_else(|| invalid("deleteRange", "`from` is required"))?;
                let to = usize_arg(args, "to")
                    .ok_or_else(|| invalid("deleteRange", "`to` is required"))?;
                Ok(delete_range(from, to))
            }),
        ));
        let s = schema;
        commands.push((
            "deleteNode".into(),
            with_args(move |args| {
                Ok(delete_node(
                    s.node_type(&type_name_arg("deleteNode", args)?)?,
                ))
            }),
        ));
        commands
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        let mut bindings: Vec<(String, Command)> = vec![
            ("Enter".into(), enter()),
            ("Mod-Enter".into(), command(pm::exit_code)),
            ("Backspace".into(), backspace()),
            ("Mod-Backspace".into(), backspace()),
            ("Shift-Backspace".into(), backspace()),
            ("Delete".into(), delete()),
            ("Mod-Delete".into(), delete()),
            ("Mod-a".into(), command(pm::select_all)),
        ];
        if ctx.platform == rs_editor_commands::KeyPlatform::Apple {
            bindings.extend([
                ("Ctrl-h".to_owned(), backspace()),
                ("Alt-Backspace".to_owned(), backspace()),
                ("Ctrl-d".to_owned(), delete()),
                ("Ctrl-Alt-Backspace".to_owned(), delete()),
                ("Alt-Delete".to_owned(), delete()),
                ("Alt-d".to_owned(), delete()),
                ("Ctrl-a".to_owned(), command(pm::select_textblock_start)),
                ("Ctrl-e".to_owned(), command(pm::select_textblock_end)),
            ]);
        }
        bindings
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Undo/redo.
#[derive(Debug, Clone, Default)]
pub struct History {
    pub options: history::HistoryOptions,
}

impl Extension for History {
    fn name(&self) -> &str {
        "history"
    }

    fn commands(&self, _ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        vec![
            ("undo".into(), no_args(command(history::undo))),
            ("redo".into(), no_args(command(history::redo))),
        ]
    }

    fn keyboard_shortcuts(&self, _ctx: &ExtensionContext) -> Vec<(String, Command)> {
        vec![
            ("Mod-z".into(), command(history::undo)),
            ("Shift-Mod-z".into(), command(history::redo)),
            ("Mod-y".into(), command(history::redo)),
        ]
    }

    fn plugins(&self, _ctx: &ExtensionContext) -> Vec<Arc<dyn Plugin>> {
        vec![history::History::new(self.options)]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Text alignment for paragraphs and headings.
#[derive(Debug, Clone)]
pub struct TextAlign {
    pub types: Vec<String>,
    pub alignments: Vec<String>,
    pub default_alignment: String,
}

impl Default for TextAlign {
    fn default() -> Self {
        Self {
            types: vec!["heading".into(), "paragraph".into()],
            alignments: ["left", "center", "right", "justify"]
                .map(String::from)
                .to_vec(),
            default_alignment: "left".into(),
        }
    }
}

impl TextAlign {
    fn set(&self, alignment: &str) -> Command {
        let attrs = json_map(json!({"textAlign": alignment}));
        crate::helpers::chain(
            self.types
                .iter()
                .map(|ty| update_attributes(ty.clone(), attrs.clone()))
                .collect(),
        )
    }
}

impl Extension for TextAlign {
    fn name(&self) -> &str {
        "textAlign"
    }

    fn global_attributes(&self) -> Vec<GlobalAttribute> {
        vec![GlobalAttribute {
            types: self.types.clone(),
            attrs: vec![(
                "textAlign".into(),
                AttrSpec::with_default(self.default_alignment.as_str()),
            )],
        }]
    }

    fn commands(&self, _ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let this = self.clone();
        let set = with_args(move |args| {
            let alignment = args
                .get("alignment")
                .and_then(Value::as_str)
                .or_else(|| args.as_str())
                .unwrap_or_default();
            if !this.alignments.iter().any(|a| a == alignment) {
                return Err(invalid(
                    "setTextAlign",
                    format!("unsupported alignment `{alignment}`"),
                ));
            }
            Ok(this.set(alignment))
        });
        let unset = no_args(self.set(&self.default_alignment));
        vec![
            ("setTextAlign".into(), set),
            ("unsetTextAlign".into(), unset),
        ]
    }

    fn keyboard_shortcuts(&self, _ctx: &ExtensionContext) -> Vec<(String, Command)> {
        vec![
            ("Mod-Shift-l".into(), self.set("left")),
            ("Mod-Shift-e".into(), self.set("center")),
            ("Mod-Shift-r".into(), self.set("right")),
            ("Mod-Shift-j".into(), self.set("justify")),
        ]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Smart quotes, dashes, ellipses, arrows and fractions while typing.
#[derive(Debug, Clone, Default)]
pub struct Typography;

impl Extension for Typography {
    fn name(&self) -> &str {
        "typography"
    }

    fn input_rules(&self, _ctx: &ExtensionContext) -> Vec<InputRule> {
        let open_quote = r#"(?:^|[\s{\[(<'"\x{2018}\x{201C}])"#;
        vec![
            text_input_rule("--$", "—"),
            text_input_rule(r"\.\.\.$", "…"),
            text_input_rule(&format!(r#"{open_quote}(")$"#), "“"),
            text_input_rule(r#""$"#, "”"),
            text_input_rule(&format!("{open_quote}(')$"), "‘"),
            text_input_rule("'$", "’"),
            text_input_rule("<-$", "←"),
            text_input_rule("->$", "→"),
            text_input_rule(r"\(c\)$", "©"),
            text_input_rule(r"\(tm\)$", "™"),
            text_input_rule(r"\(r\)$", "®"),
            text_input_rule(r"(?:^|\s)(1/2)\s$", "½"),
            text_input_rule(r"(?:^|\s)(1/4)\s$", "¼"),
            text_input_rule(r"(?:^|\s)(3/4)\s$", "¾"),
            text_input_rule(r"\+/-$", "±"),
            text_input_rule("!=$", "≠"),
            text_input_rule("<<$", "«"),
            text_input_rule(">>$", "»"),
            text_input_rule(r"\^2$", "²"),
            text_input_rule(r"\^3$", "³"),
        ]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// How [`CharacterCount`] counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CountMode {
    /// User-perceived characters (grapheme clusters), so an emoji counts once.
    #[default]
    Graphemes,
    /// Document size in positions, like ProseMirror's `nodeSize`.
    NodeSize,
}

/// Counts characters and words, optionally enforcing a limit.
#[derive(Debug, Clone, Default)]
pub struct CharacterCount {
    pub limit: Option<usize>,
    pub mode: CountMode,
}

impl CharacterCount {
    pub fn characters(&self, doc: &Node) -> usize {
        match self.mode {
            CountMode::Graphemes => doc
                .text_between(0, doc.content().size(), Some("\n"), Some(" "))
                .graphemes(true)
                .filter(|g| *g != "\n")
                .count(),
            CountMode::NodeSize => doc.content().size(),
        }
    }

    pub fn words(&self, doc: &Node) -> usize {
        doc.text_between(0, doc.content().size(), Some(" "), Some(" "))
            .unicode_words()
            .count()
    }
}

impl Extension for CharacterCount {
    fn name(&self) -> &str {
        "characterCount"
    }

    fn plugins(&self, _ctx: &ExtensionContext) -> Vec<Arc<dyn Plugin>> {
        if self.limit.is_none() {
            return Vec::new();
        }
        vec![Arc::new(CharacterLimit(self.clone()))]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct CharacterLimit(CharacterCount);

impl Plugin for CharacterLimit {
    fn key(&self) -> &str {
        "characterCount$"
    }

    fn filter_transaction(&self, tr: &Transaction, _state: &EditorState) -> bool {
        let Some(limit) = self.0.limit else {
            return true;
        };
        if !tr.doc_changed() {
            return true;
        }
        // Allow anything within the limit, and edits that shrink an
        // already oversized document.
        let after = self.0.characters(&tr.doc);
        after <= limit || after <= self.0.characters(tr.before())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Keeps a paragraph at the end of the document when it would otherwise
/// end with something that is not a textblock (an image, a rule, a list,
/// a quote), so there is always a line to click into and type below it,
/// like Tiptap's TrailingNode.
#[derive(Debug, Clone, Default)]
pub struct TrailingNode;

impl Extension for TrailingNode {
    fn name(&self) -> &str {
        "trailingNode"
    }

    fn plugins(&self, _ctx: &ExtensionContext) -> Vec<Arc<dyn Plugin>> {
        vec![Arc::new(TrailingParagraph)]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct TrailingParagraph;

impl Plugin for TrailingParagraph {
    fn key(&self) -> &str {
        "trailingNode$"
    }

    fn append_transaction(
        &self,
        transactions: &[Transaction],
        _old: &EditorState,
        new: &EditorState,
    ) -> Option<Transaction> {
        // Edits only: content set as a whole stays exactly as given.
        if !transactions.iter().any(|tr| tr.doc_changed())
            || transactions.iter().any(|tr| {
                tr.get_meta::<bool>(crate::core_commands::SET_CONTENT)
                    .is_some()
            })
        {
            return None;
        }
        let doc = new.doc();
        if doc.last_child().is_none_or(|last| last.is_textblock()) {
            return None;
        }
        let paragraph = new.schema().node_type("paragraph").ok()?;
        let empty = paragraph
            .create(None, rs_editor_model::Fragment::empty(), Default::default())
            .ok()?;
        let mut tr = new.tr();
        tr.insert(
            doc.content().size(),
            rs_editor_model::Fragment::from_node(empty),
        )
        .ok()?;
        Some(tr)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Placeholder text for empty documents or empty blocks. The view draws
/// it; this extension only holds the configuration.
#[derive(Debug, Clone)]
pub struct Placeholder {
    /// Text shown in an empty document.
    pub text: String,
    /// Text per node type for empty blocks, e.g. `("heading", "Heading")`.
    pub per_type: Vec<(String, String)>,
    /// Show the placeholder in every empty block, not just the current one.
    pub show_only_current: bool,
    /// Show placeholders in an empty document only.
    pub empty_document_only: bool,
}

impl Default for Placeholder {
    fn default() -> Self {
        Self {
            text: Self::DEFAULT_TEXT.into(),
            per_type: Vec::new(),
            show_only_current: true,
            empty_document_only: false,
        }
    }
}

impl Placeholder {
    /// The text when the app sets none (views may show a localized one).
    pub const DEFAULT_TEXT: &str = "Write something…";

    /// The placeholder to show for the empty textblock `node`, if any.
    pub fn text_for(&self, node: &Node, doc_is_empty: bool) -> Option<&str> {
        if !node.is_textblock()
            || node.content().size() > 0
            || (self.empty_document_only && !doc_is_empty)
        {
            return None;
        }
        self.per_type
            .iter()
            .find(|(ty, _)| ty == node.type_name())
            .map(|(_, text)| text.as_str())
            .or(Some(self.text.as_str()))
    }
}

impl Extension for Placeholder {
    fn name(&self) -> &str {
        "placeholder"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
