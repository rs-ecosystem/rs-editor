//! Input rules: markdown-style shortcuts that run while typing
//! (`**bold**`, `# `, `- `, `---`). Port of `prosemirror-inputrules` with
//! the rule constructors of Tiptap's core.
//!
//! Patterns use the `regex` crate (no lookaround) and must end with `$`;
//! they are matched against the text before the cursor plus the typed text.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use regex::Regex;
use rs_editor_model::{Fragment, MarkSet, MarkType, Node, NodeType};
use rs_editor_state::{Dispatch, EditorState, Plugin, PluginValue, Transaction, ViewContext};
use rs_editor_transform::{Assoc, Mappable, Step, can_join, find_wrapping};
use serde_json::{Map, Value};

/// Plugin key of the input rules plugin.
pub const INPUT_RULES_KEY: &str = "inputRules$";
const MAX_MATCH: usize = 500;

/// Capture groups of a rule match; index 0 is the whole match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleMatch {
    pub groups: Vec<Option<String>>,
}

impl RuleMatch {
    pub fn full(&self) -> &str {
        self.group(0).unwrap_or("")
    }

    pub fn group(&self, index: usize) -> Option<&str> {
        self.groups.get(index).and_then(|g| g.as_deref())
    }

    /// The last capture group, used by mark rules for the marked text.
    pub fn last(&self) -> Option<&str> {
        self.groups.last().and_then(|g| g.as_deref())
    }
}

/// Builds the transaction for a match covering `start..end` (the typed
/// text is not yet inserted), or `None` to skip the rule.
pub type RuleHandler =
    Arc<dyn Fn(&EditorState, &RuleMatch, usize, usize) -> Option<Transaction> + Send + Sync>;

/// Derives node or mark attributes from a match; `None` skips the rule.
pub type RuleAttrs = Arc<dyn Fn(&RuleMatch) -> Option<Map<String, Value>> + Send + Sync>;

/// Where a rule may run relative to code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InCode {
    Never,
    Also,
    Only,
}

/// A single input rule.
#[derive(Clone)]
pub struct InputRule {
    pattern: Regex,
    handler: RuleHandler,
    /// Backspace right after the rule restores the typed text.
    pub undoable: bool,
    /// Whether the rule runs in code blocks.
    pub in_code: InCode,
    /// Whether the rule runs inside code marks.
    pub in_code_mark: bool,
}

impl fmt::Debug for InputRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputRule")
            .field("pattern", &self.pattern.as_str())
            .finish()
    }
}

impl InputRule {
    /// A rule with a custom handler. Panics if `pattern` is invalid, so
    /// patterns should be literals checked by tests.
    pub fn new(pattern: &str, handler: RuleHandler) -> Self {
        Self {
            pattern: Regex::new(pattern)
                .unwrap_or_else(|error| panic!("invalid input rule {pattern}: {error}")),
            handler,
            undoable: true,
            in_code: InCode::Never,
            in_code_mark: false,
        }
    }

    pub fn pattern(&self) -> &str {
        self.pattern.as_str()
    }

    fn matches(&self, text: &str) -> Option<RuleMatch> {
        let captures = self.pattern.captures(text)?;
        Some(RuleMatch {
            groups: captures
                .iter()
                .map(|g| g.map(|m| m.as_str().to_owned()))
                .collect(),
        })
    }
}

fn utf16(text: &str) -> usize {
    rs_editor_model::text::utf16_len(text)
}

fn has_code_mark(marks: &MarkSet) -> bool {
    marks.iter().any(|m| m.mark_type().spec().code)
}

/// Runs the first matching rule for `text` typed over `from..to`.
pub fn run_input_rules(
    rules: &[InputRule],
    state: &EditorState,
    from: usize,
    to: usize,
    text: &str,
) -> Option<Transaction> {
    let rfrom = state.doc().resolve(from).ok()?;
    let parent = rfrom.parent();
    let offset = rfrom.parent_offset();
    let text_before = parent.text_between(
        offset.saturating_sub(MAX_MATCH),
        offset,
        None,
        Some("\u{fffc}"),
    ) + text;
    let in_code_block = parent.node_type().spec().code;
    for rule in rules {
        if !rule.in_code_mark && has_code_mark(&rfrom.marks()) {
            continue;
        }
        match (in_code_block, rule.in_code) {
            (true, InCode::Never) | (false, InCode::Only) => continue,
            _ => {}
        }
        let Some(found) = rule.matches(&text_before) else {
            continue;
        };
        let match_len = utf16(found.full());
        let text_len = utf16(text);
        if match_len < text_len || from + text_len < match_len {
            continue;
        }
        let start = from - (match_len - text_len);
        if !rule.in_code_mark {
            let mut has_mark = false;
            state
                .doc()
                .nodes_between(start, from, &mut |node, _, _, _| {
                    if node.is_inline() && has_code_mark(node.marks()) {
                        has_mark = true;
                    }
                    !has_mark
                });
            if has_mark {
                continue;
            }
        }
        let Some(mut tr) = (rule.handler)(state, &found, start, to) else {
            continue;
        };
        if rule.undoable {
            let steps = tr
                .steps
                .iter()
                .cloned()
                .zip(tr.docs.iter().cloned())
                .collect();
            tr.set_meta(
                INPUT_RULES_KEY,
                UndoableRule {
                    steps: Arc::new(steps),
                    from,
                    to,
                    text: text.to_owned(),
                },
            );
        }
        return Some(tr);
    }
    None
}

/// What [`undo_input_rule`] needs to revert the last rule.
#[derive(Debug, Clone)]
pub struct UndoableRule {
    steps: Arc<Vec<(Step, Node)>>,
    from: usize,
    to: usize,
    text: String,
}

/// Tracks the most recent rule application so Backspace can undo it.
#[derive(Debug, Clone, Default)]
pub struct InputRulesPlugin {
    pub rules: Vec<InputRule>,
}

impl Plugin for InputRulesPlugin {
    fn key(&self) -> &str {
        INPUT_RULES_KEY
    }

    fn init(&self, _state: &EditorState) -> Option<PluginValue> {
        Some(Arc::new(None::<UndoableRule>))
    }

    fn apply(
        &self,
        tr: &Transaction,
        value: &PluginValue,
        _old: &EditorState,
        _new: &EditorState,
    ) -> PluginValue {
        if let Some(stored) = tr.get_meta::<UndoableRule>(INPUT_RULES_KEY) {
            return Arc::new(Some(stored.clone()));
        }
        if tr.selection_set() || tr.doc_changed() {
            Arc::new(None::<UndoableRule>)
        } else {
            value.clone()
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Reverts the input rule applied by the last change (Backspace).
pub fn undo_input_rule(
    state: &EditorState,
    dispatch: Dispatch,
    _view: Option<&dyn ViewContext>,
) -> bool {
    let Some(stored) = state.plugin_state::<Option<UndoableRule>>(INPUT_RULES_KEY) else {
        return false;
    };
    let Some(undoable) = stored.as_ref() else {
        return false;
    };
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        for (step, doc) in undoable.steps.iter().rev() {
            let Ok(inverted) = step.invert(doc) else {
                return false;
            };
            if tr.step(inverted).is_err() {
                return false;
            }
        }
        let result = if undoable.text.is_empty() {
            tr.delete(undoable.from, undoable.to).map(|_| ())
        } else {
            let marks = tr
                .doc
                .resolve(undoable.from)
                .map(|pos| pos.marks())
                .unwrap_or_else(|_| MarkSet::none());
            match state.schema().text(undoable.text.as_str(), marks) {
                Ok(text) => tr
                    .replace_with(undoable.from, undoable.to, Fragment::from_node(text))
                    .map(|_| ()),
                Err(_) => return false,
            }
        };
        if result.is_err() {
            return false;
        }
        dispatch(tr);
    }
    true
}

// ---- rule constructors ---------------------------------------------------------

/// Replaces the match with `replacement`, keeping the text after the first
/// capture group (typography rules such as `--` → `—`).
pub fn text_input_rule(pattern: &str, replacement: &'static str) -> InputRule {
    InputRule::new(
        pattern,
        Arc::new(
            move |state: &EditorState, found: &RuleMatch, start: usize, end: usize| {
                let mut insert = replacement.to_owned();
                let mut start = start;
                if let Some(group) = found.group(1) {
                    let full = found.full();
                    let offset = full.rfind(group)?;
                    insert.push_str(&full[offset + group.len()..]);
                    start += utf16(&full[..offset]);
                    if start > end {
                        let cut = start - end;
                        let prefix: String = {
                            let before = &full[..offset];
                            let units: Vec<u16> = before.encode_utf16().collect();
                            String::from_utf16_lossy(&units[units.len().saturating_sub(cut)..])
                        };
                        insert = prefix + &insert;
                        start = end;
                    }
                }
                let mut tr = state.tr();
                tr.insert_text_at(&insert, start, end).ok()?;
                Some(tr)
            },
        ),
    )
}

/// Turns `**text**`-style input into marked text. The last capture group
/// is the text to mark; the markers around it are removed.
pub fn mark_input_rule(pattern: &str, mark_type: MarkType, attrs: Option<RuleAttrs>) -> InputRule {
    InputRule::new(
        pattern,
        Arc::new(
            move |state: &EditorState, found: &RuleMatch, from: usize, to: usize| {
                let attributes = match &attrs {
                    Some(attrs) => Some(attrs(found)?),
                    None => None,
                };
                let capture = found.last()?;
                if capture.is_empty() {
                    return None;
                }
                let full = found.full();
                let start_spaces = utf16(&full[..full.find(|c: char| !c.is_whitespace())?]);
                let text_start = from + utf16(&full[..full.find(capture)?]);
                let text_end = text_start + utf16(capture);
                let mut excluded = false;
                state.doc().nodes_between(from, to, &mut |node, pos, _, _| {
                    if pos + node.node_size() > text_start
                        && node.marks().iter().any(|m| {
                            m.mark_type() != &mark_type && m.mark_type().excludes(&mark_type)
                        })
                    {
                        excluded = true;
                    }
                    !excluded
                });
                if excluded {
                    return None;
                }
                let mut tr = state.tr();
                if text_end < to {
                    tr.delete(text_end, to).ok()?;
                }
                if text_start > from {
                    tr.delete(from + start_spaces, text_start).ok()?;
                }
                let mark_end = from + start_spaces + utf16(capture);
                let mark = mark_type.create(attributes.as_ref()).ok()?;
                tr.add_mark(from + start_spaces, mark_end, &mark).ok()?;
                tr.remove_stored_mark_type(&mark_type);
                Some(tr)
            },
        ),
    )
}

/// Changes the textblock to `node_type` when its start matches (`# ` → heading).
pub fn textblock_type_input_rule(
    pattern: &str,
    node_type: NodeType,
    attrs: Option<RuleAttrs>,
) -> InputRule {
    InputRule::new(
        pattern,
        Arc::new(
            move |state: &EditorState, found: &RuleMatch, from: usize, to: usize| {
                let attributes = match &attrs {
                    Some(attrs) => Some(attrs(found)?),
                    None => None,
                };
                let start = state.doc().resolve(from).ok()?;
                let depth = start.depth().checked_sub(1)?;
                if !start.node(depth).can_replace_with(
                    start.index(depth),
                    start.index_after(depth),
                    &node_type,
                    None,
                ) {
                    return None;
                }
                let mut tr = state.tr();
                tr.delete(from, to).ok()?;
                tr.set_block_type(from, from, &node_type, attributes.as_ref())
                    .ok()?;
                Some(tr)
            },
        ),
    )
}

/// Decides whether a new wrapper joins the same-typed node before it.
pub type JoinRule = Arc<dyn Fn(&RuleMatch, &Node) -> bool + Send + Sync>;

/// Wraps the textblock in `node_type` when its start matches (`- ` → bullet list).
pub fn wrapping_input_rule(
    pattern: &str,
    node_type: NodeType,
    attrs: Option<RuleAttrs>,
    join: Option<JoinRule>,
) -> InputRule {
    InputRule::new(
        pattern,
        Arc::new(
            move |state: &EditorState, found: &RuleMatch, from: usize, to: usize| {
                let attributes = match &attrs {
                    Some(attrs) => Some(attrs(found)?),
                    None => None,
                };
                let mut tr = state.tr();
                tr.delete(from, to).ok()?;
                let start = tr.doc.resolve(from).ok()?;
                let range = start.block_range(&start, None)?;
                let wrapping = find_wrapping(&range, &node_type, attributes, None)?;
                tr.wrap(&range, &wrapping).ok()?;
                let before = from
                    .checked_sub(1)
                    .and_then(|p| tr.doc.resolve(p).ok())
                    .and_then(|p| p.node_before());
                if let Some(before) = before
                    && before.node_type() == &node_type
                    && can_join(&tr.doc, from - 1)
                    && join.as_ref().is_none_or(|join| join(found, &before))
                {
                    tr.join(from - 1, 1).ok()?;
                }
                Some(tr)
            },
        ),
    )
}

/// Replaces the match with a node (`---` → horizontal rule).
pub fn node_input_rule(pattern: &str, node_type: NodeType, attrs: Option<RuleAttrs>) -> InputRule {
    InputRule::new(
        pattern,
        Arc::new(
            move |state: &EditorState, found: &RuleMatch, from: usize, to: usize| {
                let attributes = match &attrs {
                    Some(attrs) => Some(attrs(found)?),
                    None => None,
                };
                let node = node_type
                    .create(attributes.as_ref(), Fragment::empty(), MarkSet::none())
                    .ok()?;
                let mut tr = state.tr();
                if node_type.is_inline() {
                    tr.replace_with(from, to, Fragment::from_node(node)).ok()?;
                } else {
                    // Insert the block before the current textblock, then drop the typed marker.
                    let insert_at = from.checked_sub(1)?;
                    tr.insert(insert_at, Fragment::from_node(node)).ok()?;
                    let start = tr.mapping.map(from, Assoc::After);
                    let end = tr.mapping.map(to, Assoc::After);
                    tr.delete(start, end).ok()?;
                }
                tr.scroll_into_view();
                Some(tr)
            },
        ),
    )
}
