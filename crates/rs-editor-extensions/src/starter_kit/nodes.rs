//! Block and inline node extensions (Tiptap names and behavior).

use std::any::Any;
use std::sync::Arc;

use rs_editor_commands as pm;
use rs_editor_model::{AttrSpec, Fragment, MarkSet, NodeSpec};
use rs_editor_state::{Command, Dispatch, EditorState, Selection, ViewContext, command};
use serde_json::{Value, json};

use super::util::*;
use crate::core_commands::{
    clear_nodes, insert_content, set_node, toggle_list, toggle_node, toggle_wrap,
};
use crate::extension::{CommandFactory, Extension, ExtensionContext};
use crate::helpers::first;
use crate::inputrules::{
    InputRule, node_input_rule, textblock_type_input_rule, wrapping_input_rule,
};

type View<'a> = Option<&'a dyn ViewContext>;

macro_rules! extension_basics {
    ($name:literal) => {
        fn name(&self) -> &str {
            $name
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    };
}

/// The top-level `doc` node.
#[derive(Debug, Clone, Default)]
pub struct Document;

impl Extension for Document {
    extension_basics!("doc");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        vec![("doc".into(), NodeSpec::new().content("block+"))]
    }
}

/// Inline text.
#[derive(Debug, Clone, Default)]
pub struct Text;

impl Extension for Text {
    extension_basics!("text");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        vec![("text".into(), NodeSpec::new().group("inline"))]
    }
}

/// The default block.
#[derive(Debug, Clone, Default)]
pub struct Paragraph;

impl Extension for Paragraph {
    extension_basics!("paragraph");

    fn priority(&self) -> i32 {
        1000
    }

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        vec![(
            "paragraph".into(),
            NodeSpec::new().content("inline*").group("block"),
        )]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let paragraph = node(&ctx.schema, "paragraph");
        vec![(
            "setParagraph".into(),
            no_args(set_node(paragraph, Default::default())),
        )]
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        let paragraph = node(&ctx.schema, "paragraph");
        vec![("Mod-Alt-0".into(), set_node(paragraph, Default::default()))]
    }
}

/// Headings `h1`..`h6` (or the configured levels).
#[derive(Debug, Clone)]
pub struct Heading {
    pub levels: Vec<u8>,
}

impl Default for Heading {
    fn default() -> Self {
        Self {
            levels: vec![1, 2, 3, 4, 5, 6],
        }
    }
}

impl Heading {
    fn level_arg(
        &self,
        command: &str,
        args: &Value,
    ) -> Result<serde_json::Map<String, Value>, crate::ExtensionError> {
        let attrs = attrs_of(args);
        let level = attrs.get("level").and_then(Value::as_u64).unwrap_or(0) as u8;
        if !self.levels.contains(&level) {
            return Err(invalid(
                command,
                format!("level must be one of {:?}", self.levels),
            ));
        }
        Ok(attrs)
    }
}

impl Extension for Heading {
    extension_basics!("heading");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        let default_level = self.levels.first().copied().unwrap_or(1);
        vec![(
            "heading".into(),
            NodeSpec::new()
                .attr(
                    "level",
                    AttrSpec::with_default(default_level).validate("number"),
                )
                .content("inline*")
                .group("block")
                .defining(),
        )]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let heading = node(&ctx.schema, "heading");
        let paragraph = node(&ctx.schema, "paragraph");
        let set = {
            let this = self.clone();
            let heading = heading.clone();
            with_args(move |args| {
                Ok(set_node(
                    heading.clone(),
                    this.level_arg("setHeading", args)?,
                ))
            })
        };
        let toggle = {
            let this = self.clone();
            with_args(move |args| {
                Ok(toggle_node(
                    heading.clone(),
                    paragraph.clone(),
                    this.level_arg("toggleHeading", args)?,
                ))
            })
        };
        vec![("setHeading".into(), set), ("toggleHeading".into(), toggle)]
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        let heading = node(&ctx.schema, "heading");
        let paragraph = node(&ctx.schema, "paragraph");
        self.levels
            .iter()
            .map(|level| {
                (
                    format!("Mod-Alt-{level}"),
                    toggle_node(
                        heading.clone(),
                        paragraph.clone(),
                        json_map(json!({"level": level})),
                    ),
                )
            })
            .collect()
    }

    fn input_rules(&self, ctx: &ExtensionContext) -> Vec<InputRule> {
        let heading = node(&ctx.schema, "heading");
        let max = self.levels.iter().max().copied().unwrap_or(6);
        let levels = self.levels.clone();
        vec![textblock_type_input_rule(
            &format!("^(#{{1,{max}}})\\s$"),
            heading,
            Some(rule_attrs(move |found| {
                let level = found.group(1)?.len() as u8;
                levels
                    .contains(&level)
                    .then(|| json_map(json!({"level": level})))
            })),
        )]
    }
}

/// Block quotes.
#[derive(Debug, Clone, Default)]
pub struct Blockquote;

impl Extension for Blockquote {
    extension_basics!("blockquote");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        vec![(
            "blockquote".into(),
            NodeSpec::new().content("block+").group("block").defining(),
        )]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let quote = node(&ctx.schema, "blockquote");
        vec![
            (
                "setBlockquote".into(),
                no_args(pm::wrap_in(quote.clone(), None)),
            ),
            (
                "toggleBlockquote".into(),
                no_args(toggle_wrap(quote.clone(), Default::default())),
            ),
            ("unsetBlockquote".into(), no_args(command(pm::lift))),
        ]
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        vec![(
            "Mod-Shift-b".into(),
            toggle_wrap(node(&ctx.schema, "blockquote"), Default::default()),
        )]
    }

    fn input_rules(&self, ctx: &ExtensionContext) -> Vec<InputRule> {
        vec![wrapping_input_rule(
            r"^\s*>\s$",
            node(&ctx.schema, "blockquote"),
            None,
            None,
        )]
    }
}

/// Code blocks with an optional language.
#[derive(Debug, Clone)]
pub struct CodeBlock {
    /// Three Enters at the end leave the block.
    pub exit_on_triple_enter: bool,
    /// Arrow down at the end of the last block creates a paragraph after it.
    pub exit_on_arrow_down: bool,
    pub default_language: Option<String>,
}

impl Default for CodeBlock {
    fn default() -> Self {
        Self {
            exit_on_triple_enter: true,
            exit_on_arrow_down: true,
            default_language: None,
        }
    }
}

fn code_block_backspace(state: &EditorState, dispatch: Dispatch, view: View) -> bool {
    let selection = state.selection();
    let Some(cursor) = selection.cursor() else {
        return false;
    };
    let at_start = Selection::at_start(state.doc()).from() == cursor.pos();
    if !cursor.parent().node_type().spec().code
        || (!cursor.parent().text_content().is_empty() && !at_start)
    {
        return false;
    }
    if cursor.parent().content().size() > 0 && cursor.parent_offset() > 0 {
        return false;
    }
    clear_nodes(state, dispatch, view)
}

fn code_block_triple_enter(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let Some(cursor) = state.selection().cursor() else {
        return false;
    };
    let parent = cursor.parent();
    if !parent.node_type().spec().code || cursor.parent_offset() != parent.content().size() {
        return false;
    }
    if !parent.text_content().ends_with("\n\n") {
        return false;
    }
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        if tr.delete(cursor.pos() - 2, cursor.pos()).is_err() {
            return false;
        }
        let Ok(resolved) = tr.doc.resolve(cursor.pos() - 2) else {
            return false;
        };
        let after = resolved.after(resolved.depth());
        let Some(paragraph) = resolved
            .node(resolved.depth() - 1)
            .content_match_at(resolved.index_after(resolved.depth() - 1))
            .ok()
            .and_then(|m| m.default_type())
            .and_then(|ty| ty.create_and_fill(None, Fragment::empty(), MarkSet::none()))
        else {
            return false;
        };
        if tr.insert(after, Fragment::from_node(paragraph)).is_err() {
            return false;
        }
        if let Ok(target) = tr.doc.resolve(after + 1) {
            let near = Selection::near(&target, 1);
            tr.set_selection(near);
        }
        tr.scroll_into_view();
        dispatch(tr);
    }
    true
}

fn code_block_arrow_down(state: &EditorState, dispatch: Dispatch, _view: View) -> bool {
    let Some(cursor) = state.selection().cursor() else {
        return false;
    };
    let parent = cursor.parent();
    let doc = state.doc();
    if !parent.node_type().spec().code
        || cursor.parent_offset() != parent.content().size()
        || cursor.depth() != 1
        || cursor.index(0) + 1 != doc.child_count()
    {
        return false;
    }
    let after = cursor.after(cursor.depth());
    let Some(ty) = doc
        .content_match_at(doc.child_count())
        .ok()
        .and_then(|m| m.default_type())
    else {
        return false;
    };
    if let Some(dispatch) = dispatch {
        let Some(paragraph) = ty.create_and_fill(None, Fragment::empty(), MarkSet::none()) else {
            return false;
        };
        let mut tr = state.tr();
        if tr.insert(after, Fragment::from_node(paragraph)).is_err() {
            return false;
        }
        if let Ok(target) = tr.doc.resolve(after + 1) {
            let near = Selection::near(&target, 1);
            tr.set_selection(near);
        }
        dispatch(tr);
    }
    true
}

impl Extension for CodeBlock {
    extension_basics!("codeBlock");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        let default_language = self
            .default_language
            .clone()
            .map_or(Value::Null, Value::String);
        vec![(
            "codeBlock".into(),
            NodeSpec::new()
                .attr("language", AttrSpec::with_default(default_language))
                .content("text*")
                .marks("")
                .group("block")
                .code()
                .defining(),
        )]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let code = node(&ctx.schema, "codeBlock");
        let paragraph = node(&ctx.schema, "paragraph");
        let set = {
            let code = code.clone();
            with_args(move |args| Ok(set_node(code.clone(), attrs_of(args))))
        };
        let toggle =
            with_args(move |args| Ok(toggle_node(code.clone(), paragraph.clone(), attrs_of(args))));
        vec![
            ("setCodeBlock".into(), set),
            ("toggleCodeBlock".into(), toggle),
        ]
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        let code = node(&ctx.schema, "codeBlock");
        let paragraph = node(&ctx.schema, "paragraph");
        let mut bindings: Vec<(String, Command)> = vec![
            (
                "Mod-Alt-c".into(),
                toggle_node(code, paragraph, Default::default()),
            ),
            ("Backspace".into(), command(code_block_backspace)),
        ];
        if self.exit_on_triple_enter {
            bindings.push(("Enter".into(), command(code_block_triple_enter)));
        }
        if self.exit_on_arrow_down {
            bindings.push(("ArrowDown".into(), command(code_block_arrow_down)));
        }
        bindings
    }

    fn input_rules(&self, ctx: &ExtensionContext) -> Vec<InputRule> {
        let code = node(&ctx.schema, "codeBlock");
        let language = || {
            rule_attrs(|found| {
                Some(json_map(json!({
                    "language": found.group(1).map_or(Value::Null, |l| Value::String(l.to_owned()))
                })))
            })
        };
        vec![
            textblock_type_input_rule(r"^```([a-z]+)?[\s\n]$", code.clone(), Some(language())),
            textblock_type_input_rule(r"^~~~([a-z]+)?[\s\n]$", code, Some(language())),
        ]
    }
}

/// Horizontal rules.
#[derive(Debug, Clone, Default)]
pub struct HorizontalRule;

impl Extension for HorizontalRule {
    extension_basics!("horizontalRule");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        vec![("horizontalRule".into(), NodeSpec::new().group("block"))]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let rule = node(&ctx.schema, "horizontalRule");
        let set: Command = Arc::new(move |state: &EditorState, dispatch: Dispatch, view: View| {
            let Ok(hr) = rule.create(None, Fragment::empty(), MarkSet::none()) else {
                return false;
            };
            insert_content(vec![hr])(state, dispatch, view)
        });
        vec![("setHorizontalRule".into(), no_args(set))]
    }

    fn input_rules(&self, ctx: &ExtensionContext) -> Vec<InputRule> {
        vec![node_input_rule(
            r"^(?:---|—-|___\s|\*\*\*\s)$",
            node(&ctx.schema, "horizontalRule"),
            None,
        )]
    }
}

/// Line breaks inside a block (Shift-Enter).
#[derive(Debug, Clone)]
pub struct HardBreak {
    /// Keep the active marks after the break.
    pub keep_marks: bool,
}

impl Default for HardBreak {
    fn default() -> Self {
        Self { keep_marks: true }
    }
}

impl Extension for HardBreak {
    extension_basics!("hardBreak");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        vec![(
            "hardBreak".into(),
            NodeSpec::new()
                .inline()
                .group("inline")
                .selectable(false)
                .leaf_text("\n")
                .linebreak_replacement(),
        )]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        vec![(
            "setHardBreak".into(),
            no_args(set_hard_break(ctx, self.keep_marks)),
        )]
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        let set = set_hard_break(ctx, self.keep_marks);
        vec![
            ("Mod-Enter".into(), set.clone()),
            ("Shift-Enter".into(), set),
        ]
    }
}

fn set_hard_break(ctx: &ExtensionContext, keep_marks: bool) -> Command {
    let brk = node(&ctx.schema, "hardBreak");
    first(vec![
        command(pm::exit_code),
        Arc::new(
            move |state: &EditorState, dispatch: Dispatch, _view: View| {
                let selection = state.selection();
                if selection.from_pos().parent().node_type().spec().code {
                    return false;
                }
                let Some(dispatch) = dispatch else {
                    return true;
                };
                let marks = state
                    .stored_marks()
                    .cloned()
                    .unwrap_or_else(|| selection.to_pos().marks());
                let Ok(node) = brk.create(None, Fragment::empty(), MarkSet::none()) else {
                    return false;
                };
                let mut tr = state.tr();
                if tr.replace_selection_with(node, false).is_err() {
                    return false;
                }
                if keep_marks && !marks.is_empty() {
                    tr.ensure_marks(marks);
                }
                tr.scroll_into_view();
                dispatch(tr);
                true
            },
        ),
    ])
}

fn list_item_bindings(ctx: &ExtensionContext, item: &str) -> Vec<(String, Command)> {
    let item = node(&ctx.schema, item);
    vec![
        ("Enter".into(), pm::split_list_item(item.clone(), None)),
        ("Tab".into(), pm::sink_list_item(item.clone())),
        ("Shift-Tab".into(), pm::lift_list_item(item)),
    ]
}

/// Items of bullet and ordered lists.
#[derive(Debug, Clone, Default)]
pub struct ListItem;

impl Extension for ListItem {
    extension_basics!("listItem");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        vec![(
            "listItem".into(),
            NodeSpec::new().content("paragraph block*").defining(),
        )]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let item = node(&ctx.schema, "listItem");
        vec![
            (
                "splitListItem".into(),
                no_args(pm::split_list_item(item.clone(), None)),
            ),
            (
                "sinkListItem".into(),
                no_args(pm::sink_list_item(item.clone())),
            ),
            ("liftListItem".into(), no_args(pm::lift_list_item(item))),
        ]
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        list_item_bindings(ctx, "listItem")
    }
}

/// Bullet lists.
#[derive(Debug, Clone, Default)]
pub struct BulletList;

impl Extension for BulletList {
    extension_basics!("bulletList");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        vec![(
            "bulletList".into(),
            NodeSpec::new().content("listItem+").group("block list"),
        )]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let list = node(&ctx.schema, "bulletList");
        let item = node(&ctx.schema, "listItem");
        vec![(
            "toggleBulletList".into(),
            no_args(toggle_list(list, item, Default::default())),
        )]
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        let list = node(&ctx.schema, "bulletList");
        let item = node(&ctx.schema, "listItem");
        vec![(
            "Mod-Shift-8".into(),
            toggle_list(list, item, Default::default()),
        )]
    }

    fn input_rules(&self, ctx: &ExtensionContext) -> Vec<InputRule> {
        vec![wrapping_input_rule(
            r"^\s*([-+*])\s$",
            node(&ctx.schema, "bulletList"),
            None,
            None,
        )]
    }
}

/// Ordered lists with a start number.
#[derive(Debug, Clone, Default)]
pub struct OrderedList;

impl Extension for OrderedList {
    extension_basics!("orderedList");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        vec![(
            "orderedList".into(),
            NodeSpec::new()
                .attr("start", AttrSpec::with_default(1).validate("number"))
                .attr("type", AttrSpec::with_default(Value::Null))
                .content("listItem+")
                .group("block list"),
        )]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let list = node(&ctx.schema, "orderedList");
        let item = node(&ctx.schema, "listItem");
        vec![(
            "toggleOrderedList".into(),
            no_args(toggle_list(list, item, Default::default())),
        )]
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        let list = node(&ctx.schema, "orderedList");
        let item = node(&ctx.schema, "listItem");
        vec![(
            "Mod-Shift-7".into(),
            toggle_list(list, item, Default::default()),
        )]
    }

    fn input_rules(&self, ctx: &ExtensionContext) -> Vec<InputRule> {
        vec![wrapping_input_rule(
            r"^(\d+)\.\s$",
            node(&ctx.schema, "orderedList"),
            Some(rule_attrs(|found| {
                let start: u64 = found.group(1)?.parse().ok()?;
                Some(json_map(json!({"start": start})))
            })),
            Some(Arc::new(|found, list| {
                let start = list.attr("start").and_then(Value::as_u64).unwrap_or(1);
                let typed: u64 = found.group(1).and_then(|n| n.parse().ok()).unwrap_or(0);
                list.child_count() as u64 + start == typed
            })),
        )]
    }
}

/// Checklists.
#[derive(Debug, Clone, Default)]
pub struct TaskList;

impl Extension for TaskList {
    extension_basics!("taskList");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        vec![(
            "taskList".into(),
            NodeSpec::new().content("taskItem+").group("block list"),
        )]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let list = node(&ctx.schema, "taskList");
        let item = node(&ctx.schema, "taskItem");
        vec![(
            "toggleTaskList".into(),
            no_args(toggle_list(list, item, Default::default())),
        )]
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        let list = node(&ctx.schema, "taskList");
        let item = node(&ctx.schema, "taskItem");
        vec![(
            "Mod-Shift-9".into(),
            toggle_list(list, item, Default::default()),
        )]
    }
}

/// Checklist items with a `checked` attribute.
#[derive(Debug, Clone)]
pub struct TaskItem {
    /// Items may contain nested lists.
    pub nested: bool,
}

impl Default for TaskItem {
    fn default() -> Self {
        Self { nested: true }
    }
}

impl Extension for TaskItem {
    extension_basics!("taskItem");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        let content = if self.nested {
            "paragraph block*"
        } else {
            "paragraph+"
        };
        vec![(
            "taskItem".into(),
            NodeSpec::new()
                .attr("checked", AttrSpec::with_default(false).validate("boolean"))
                .content(content)
                .defining(),
        )]
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        let mut bindings = list_item_bindings(ctx, "taskItem");
        if !self.nested {
            bindings.retain(|(key, _)| key != "Tab");
        }
        bindings
    }

    fn input_rules(&self, ctx: &ExtensionContext) -> Vec<InputRule> {
        vec![wrapping_input_rule(
            r"^\s*(\[([ x])?\])\s$",
            node(&ctx.schema, "taskItem"),
            Some(rule_attrs(|found| {
                Some(json_map(json!({"checked": found.group(2) == Some("x")})))
            })),
            None,
        )]
    }
}

/// Images (block-level by default).
#[derive(Debug, Clone, Default)]
pub struct Image {
    pub inline: bool,
}

/// After inserting a block image (which selects it), the caret goes to
/// the textblock after it, a new paragraph when there is none: typing on
/// does not replace the image, and the next image goes below this one.
fn caret_after_block() -> Command {
    Arc::new(
        |state: &EditorState, dispatch: Dispatch, _view: Option<&dyn ViewContext>| {
            let selection = state.selection();
            let Selection::Node(node) = selection else {
                return true;
            };
            if !node.node().is_leaf() || node.node().is_inline() {
                return true;
            }
            if let Some(dispatch) = dispatch {
                let after = selection.to();
                let mut tr = state.tr();
                let next_is_text = tr
                    .doc
                    .resolve(after)
                    .ok()
                    .and_then(|at| at.node_after())
                    .is_some_and(|next| next.is_textblock());
                if !next_is_text {
                    let Ok(paragraph) = state.schema().node_type("paragraph") else {
                        return true;
                    };
                    let Ok(empty) = paragraph.create(None, Fragment::empty(), MarkSet::none())
                    else {
                        return true;
                    };
                    if tr.insert(after, Fragment::from_node(empty)).is_err() {
                        return true;
                    }
                }
                if let Ok(caret) = tr.doc.resolve(after + 1) {
                    tr.set_selection(Selection::near(&caret, 1));
                }
                dispatch(tr);
            }
            true
        },
    )
}

impl Extension for Image {
    extension_basics!("image");

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        let mut spec = NodeSpec::new()
            .attr("src", AttrSpec::with_default(Value::Null))
            .attr("alt", AttrSpec::with_default(Value::Null))
            .attr("title", AttrSpec::with_default(Value::Null))
            .attr("width", AttrSpec::with_default(Value::Null))
            .attr("height", AttrSpec::with_default(Value::Null))
            .draggable();
        spec = if self.inline {
            spec.inline().group("inline")
        } else {
            spec.group("block")
        };
        vec![("image".into(), spec)]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let image = node(&ctx.schema, "image");
        let set = with_args(move |args| {
            let attrs = attrs_of(args);
            if !attrs.get("src").is_some_and(Value::is_string) {
                return Err(invalid("setImage", "`src` must be a string"));
            }
            let node = image
                .create(Some(&attrs), Fragment::empty(), MarkSet::none())
                .map_err(crate::ExtensionError::Schema)?;
            Ok(crate::helpers::chain(vec![
                insert_content(vec![node]),
                caret_after_block(),
            ]))
        });
        vec![("setImage".into(), set)]
    }

    fn input_rules(&self, ctx: &ExtensionContext) -> Vec<InputRule> {
        vec![node_input_rule(
            r#"(?:^|\s)(!\[(.*)\]\((\S+)(?:\s+["'](\S+)["'])?\))$"#,
            node(&ctx.schema, "image"),
            Some(rule_attrs(|found| {
                Some(json_map(json!({
                    "alt": found.group(2),
                    "src": found.group(3),
                    "title": found.group(4),
                })))
            })),
        )]
    }
}

/// Toggles the `checked` attribute of the task item at `pos` (checkbox clicks).
pub fn toggle_task_item(pos: usize) -> Command {
    Arc::new(
        move |state: &EditorState, dispatch: Dispatch, _view: View| {
            let Some(item) = state.doc().node_at(pos) else {
                return false;
            };
            if item.type_name() != "taskItem" {
                return false;
            }
            if let Some(dispatch) = dispatch {
                let checked = item
                    .attr("checked")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut tr = state.tr();
                if tr
                    .set_node_attribute(pos, "checked", Value::Bool(!checked))
                    .is_err()
                {
                    return false;
                }
                dispatch(tr);
            }
            true
        },
    )
}
