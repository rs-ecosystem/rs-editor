//! Mark extensions (Tiptap names and behavior).

use std::any::Any;
use std::sync::Arc;

use rs_editor_model::{AttrSpec, MarkSpec, MarkType};
use rs_editor_state::{Command, Dispatch, EditorState, Plugin, Transaction, ViewContext};
use serde_json::{Map, Value, json};

use super::util::*;
use crate::core_commands::{set_mark, toggle_mark, unset_mark};
use crate::extension::{CommandFactory, Extension, ExtensionContext};
use crate::helpers::chain;
use crate::inputrules::{InputRule, mark_input_rule};

/// Delimited `inner` text that does not start or end with whitespace,
/// preceded by the start or whitespace: `(?:^|\s)(D(inner)D)$`.
fn delimited(delimiter: &str, forbidden: char) -> String {
    let d = regex::escape(delimiter);
    let f = regex::escape(&forbidden.to_string());
    format!(r"(?:^|\s)({d}([^{f}\s]|[^{f}\s][^{f}]*[^{f}\s]){d})$")
}

/// set/toggle/unset commands named after the mark, e.g. `toggleBold`.
fn mark_commands(mark_type: &MarkType, suffix: &str) -> Vec<(String, CommandFactory)> {
    let set = {
        let ty = mark_type.clone();
        with_args(move |args| Ok(set_mark(ty.clone(), attrs_of(args))))
    };
    let toggle = {
        let ty = mark_type.clone();
        with_args(move |args| Ok(toggle_mark(ty.clone(), attrs_of(args), false)))
    };
    let unset = no_args(unset_mark(mark_type.clone(), false));
    vec![
        (format!("set{suffix}"), set),
        (format!("toggle{suffix}"), toggle),
        (format!("unset{suffix}"), unset),
    ]
}

macro_rules! simple_mark {
    ($ty:ident, $name:literal, $suffix:literal, $spec:expr, [$($key:literal),*], [$(($delim:literal, $forbid:literal)),*]) => {
        #[derive(Debug, Clone, Default)]
        pub struct $ty;

        impl Extension for $ty {
            fn name(&self) -> &str {
                $name
            }

            fn marks(&self) -> Vec<(String, MarkSpec)> {
                vec![($name.into(), $spec)]
            }

            fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
                mark_commands(&mark(&ctx.schema, $name), $suffix)
            }

            fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
                let ty = mark(&ctx.schema, $name);
                vec![$(($key.into(), toggle_mark(ty.clone(), Map::new(), false))),*]
            }

            fn input_rules(&self, ctx: &ExtensionContext) -> Vec<InputRule> {
                let _ty = mark(&ctx.schema, $name);
                vec![$(mark_input_rule(&delimited($delim, $forbid), _ty.clone(), None)),*]
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }
    };
}

simple_mark!(
    Bold,
    "bold",
    "Bold",
    MarkSpec::new(),
    ["Mod-b", "Mod-B"],
    [("**", '*'), ("__", '_')]
);
simple_mark!(
    Italic,
    "italic",
    "Italic",
    MarkSpec::new(),
    ["Mod-i", "Mod-I"],
    [("*", '*'), ("_", '_')]
);
simple_mark!(
    Underline,
    "underline",
    "Underline",
    MarkSpec::new(),
    ["Mod-u", "Mod-U"],
    []
);
simple_mark!(
    Strike,
    "strike",
    "Strike",
    MarkSpec::new(),
    ["Mod-Shift-s"],
    [("~~", '~')]
);
simple_mark!(
    Code,
    "code",
    "Code",
    MarkSpec::new().excludes("_").code(),
    ["Mod-e"],
    [("`", '`')]
);
simple_mark!(
    Subscript,
    "subscript",
    "Subscript",
    MarkSpec::new().excludes("superscript"),
    ["Mod-,"],
    []
);
simple_mark!(
    Superscript,
    "superscript",
    "Superscript",
    MarkSpec::new().excludes("subscript"),
    ["Mod-."],
    []
);

/// Text highlights, optionally with a color.
#[derive(Debug, Clone, Default)]
pub struct Highlight {
    /// Allow a `color` attribute per highlight.
    pub multicolor: bool,
}

impl Extension for Highlight {
    fn name(&self) -> &str {
        "highlight"
    }

    fn marks(&self) -> Vec<(String, MarkSpec)> {
        let mut spec = MarkSpec::new();
        if self.multicolor {
            spec = spec.attr("color", AttrSpec::with_default(Value::Null));
        }
        vec![("highlight".into(), spec)]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        mark_commands(&mark(&ctx.schema, "highlight"), "Highlight")
    }

    fn keyboard_shortcuts(&self, ctx: &ExtensionContext) -> Vec<(String, Command)> {
        vec![(
            "Mod-Shift-h".into(),
            toggle_mark(mark(&ctx.schema, "highlight"), Map::new(), false),
        )]
    }

    fn input_rules(&self, ctx: &ExtensionContext) -> Vec<InputRule> {
        vec![mark_input_rule(
            &delimited("==", '='),
            mark(&ctx.schema, "highlight"),
            None,
        )]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Hyperlinks.
#[derive(Debug, Clone)]
pub struct Link {
    /// Turn typed URLs into links when a space or Enter follows them.
    pub autolink: bool,
    /// Typing at the end of a link extends it.
    pub inclusive: bool,
    /// Allowed URL schemes; other `href`s are rejected.
    pub protocols: Vec<String>,
    pub default_target: Option<String>,
}

impl Default for Link {
    fn default() -> Self {
        Self {
            autolink: true,
            inclusive: false,
            protocols: ["http", "https", "mailto", "tel"]
                .map(String::from)
                .to_vec(),
            default_target: None,
        }
    }
}

/// Meta key: the change already set links explicitly; skip autolinking.
pub const PREVENT_AUTOLINK: &str = "preventAutolink";

impl Link {
    /// Whether `href` uses an allowed scheme (relative and anchor links pass).
    pub fn is_allowed(&self, href: &str) -> bool {
        let href = href.trim();
        if href.is_empty() {
            return false;
        }
        match href.split_once(':') {
            Some((scheme, _))
                if scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)) =>
            {
                self.protocols
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(scheme))
            }
            _ => true,
        }
    }
}

fn prevent_autolink() -> Command {
    Arc::new(
        |state: &EditorState, dispatch: Dispatch, _view: Option<&dyn ViewContext>| {
            if let Some(dispatch) = dispatch {
                let mut tr = state.tr();
                tr.set_meta(PREVENT_AUTOLINK, true);
                dispatch(tr);
            }
            true
        },
    )
}

impl Extension for Link {
    fn name(&self) -> &str {
        "link"
    }

    fn priority(&self) -> i32 {
        1000
    }

    fn marks(&self) -> Vec<(String, MarkSpec)> {
        let target = self
            .default_target
            .clone()
            .map_or(Value::Null, Value::String);
        vec![(
            "link".into(),
            MarkSpec::new()
                .attr("href", AttrSpec::with_default(Value::Null))
                .attr("target", AttrSpec::with_default(target))
                .attr(
                    "rel",
                    AttrSpec::with_default("noopener noreferrer nofollow"),
                )
                .attr("class", AttrSpec::with_default(Value::Null))
                .inclusive(self.inclusive),
        )]
    }

    fn commands(&self, ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        let link = mark(&ctx.schema, "link");
        let set = {
            let this = self.clone();
            let link = link.clone();
            with_args(move |args| {
                let attrs = attrs_of(args);
                let href = attrs.get("href").and_then(Value::as_str).unwrap_or("");
                if !this.is_allowed(href) {
                    return Err(invalid(
                        "setLink",
                        format!("`{href}` is not an allowed link"),
                    ));
                }
                Ok(chain(vec![
                    set_mark(link.clone(), attrs),
                    prevent_autolink(),
                ]))
            })
        };
        let toggle = {
            let this = self.clone();
            let link = link.clone();
            with_args(move |args| {
                let attrs = attrs_of(args);
                let href = attrs.get("href").and_then(Value::as_str).unwrap_or("");
                if !this.is_allowed(href) {
                    return Err(invalid(
                        "toggleLink",
                        format!("`{href}` is not an allowed link"),
                    ));
                }
                Ok(chain(vec![
                    toggle_mark(link.clone(), attrs, true),
                    prevent_autolink(),
                ]))
            })
        };
        let unset = no_args(chain(vec![unset_mark(link, true), prevent_autolink()]));
        vec![
            ("setLink".into(), set),
            ("toggleLink".into(), toggle),
            ("unsetLink".into(), unset),
        ]
    }

    fn plugins(&self, ctx: &ExtensionContext) -> Vec<Arc<dyn Plugin>> {
        if !self.autolink {
            return Vec::new();
        }
        vec![Arc::new(Autolink {
            link: mark(&ctx.schema, "link"),
            options: self.clone(),
        })]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Marks a URL as a link when the user types whitespace after it.
struct Autolink {
    link: MarkType,
    options: Link,
}

/// Whether `tr` inserted text that ends with whitespace.
fn typed_whitespace(tr: &Transaction) -> bool {
    tr.steps.iter().any(|step| match step {
        rs_editor_transform::Step::Replace(replace) => replace
            .slice
            .content
            .last_child()
            .and_then(|node| node.text())
            .is_some_and(|text| text.ends_with(char::is_whitespace)),
        _ => false,
    })
}

/// Whether `word` looks like a web or mail address.
pub fn detect_link(word: &str) -> Option<String> {
    let trimmed = word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\'']);
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return (trimmed.len() > lower.find("//")? + 3).then(|| trimmed.to_owned());
    }
    if lower.starts_with("www.") && trimmed.len() > 6 && trimmed[4..].contains('.') {
        return Some(format!("https://{trimmed}"));
    }
    if let Some((user, domain)) = trimmed.split_once('@')
        && !user.is_empty()
        && !user.contains(['/', ':'])
        && domain.contains('.')
        && !domain.ends_with('.')
    {
        return Some(format!("mailto:{trimmed}"));
    }
    None
}

impl Plugin for Autolink {
    fn key(&self) -> &str {
        "autolink$"
    }

    fn append_transaction(
        &self,
        trs: &[Transaction],
        _old: &EditorState,
        new: &EditorState,
    ) -> Option<Transaction> {
        // Only right after the user typed whitespace; never for undo/redo,
        // pastes or programmatic changes that happen to end with a space.
        if !trs.iter().any(typed_whitespace)
            || trs.iter().any(|tr| {
                tr.has_meta(PREVENT_AUTOLINK) || rs_editor_history::is_history_transaction(tr)
            })
        {
            return None;
        }
        let cursor = new.selection().cursor()?.clone();
        let parent = cursor.parent();
        if parent.node_type().spec().code {
            return None;
        }
        let before = parent.text_between(0, cursor.parent_offset(), None, Some("\u{fffc}"));
        // Only act right after a word boundary was typed.
        let last = before.chars().last()?;
        if !last.is_whitespace() {
            return None;
        }
        let without_space = &before[..before.len() - last.len_utf8()];
        let word_start = without_space
            .rfind(char::is_whitespace)
            .map_or(0, |i| i + 1);
        let word = &without_space[word_start..];
        let href = detect_link(word)?;
        if !self.options.is_allowed(&href) {
            return None;
        }
        let text_start = cursor.start(cursor.depth());
        let from = text_start + rs_editor_model::text::utf16_len(&without_space[..word_start]);
        let to = from
            + rs_editor_model::text::utf16_len(
                word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\'']),
            );
        if new.doc().range_has_mark(from, to, &self.link) {
            return None;
        }
        let mark = self
            .link
            .create(Some(&json_map(json!({"href": href}))))
            .ok()?;
        let mut tr = new.tr();
        tr.add_mark(from, to, &mark).ok()?;
        Some(tr)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl std::fmt::Debug for Autolink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Autolink").finish()
    }
}
