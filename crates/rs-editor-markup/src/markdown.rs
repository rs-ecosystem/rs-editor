//! Markdown in and out, as GitHub writes it (CommonMark with strikethrough,
//! task lists and tables).
//!
//! Parsing goes Markdown → HTML (pulldown-cmark) → [`parse_fragment`], so
//! any Markdown gives valid content, the same way pasted HTML does.
//! Serializing writes Markdown directly in prosemirror-markdown's manner;
//! marks without Markdown syntax (underline, highlight, subscript,
//! superscript) are written as inline HTML, which Markdown allows and the
//! parser reads back.
//!
//! What Markdown cannot say is lost on the way: text alignment, link
//! targets, highlight colors, empty paragraphs, and marks on spaces at the
//! edges of marked text (those spaces end up outside the delimiters).

use pulldown_cmark::{Options, Parser, html};
use rs_editor_model::{Fragment, Mark, Node, Schema, fragment_to_json};
use serde_json::Value;

use crate::parse_fragment;

/// Markdown → content valid in `schema`.
pub fn parse_markdown(schema: &Schema, markdown: &str) -> Fragment {
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
    let mut html = String::new();
    html::push_html(&mut html, Parser::new_ext(markdown, options));
    // Code blocks come out with the newline before the closing fence.
    let html = html.replace("\n</code></pre>", "</code></pre>");
    let fragment = parse_fragment(schema, &html);
    // `&nbsp;` is how spaces Markdown would drop are written (see
    // `push_text`); read them back as the spaces they were.
    let mut json = fragment_to_json(&fragment);
    if !replace_nbsp(&mut json) {
        return fragment;
    }
    schema.fragment_from_json(&json).unwrap_or(fragment)
}

/// Turns no-break spaces in text nodes into spaces; whether any changed.
fn replace_nbsp(value: &mut Value) -> bool {
    match value {
        Value::Array(items) => items
            .iter_mut()
            .fold(false, |any, item| replace_nbsp(item) | any),
        Value::Object(object) => {
            let mut changed = false;
            if let Some(Value::String(text)) = object.get_mut("text")
                && text.contains('\u{a0}')
            {
                *text = text.replace('\u{a0}', " ");
                changed = true;
            }
            if let Some(content) = object.get_mut("content") {
                changed |= replace_nbsp(content);
            }
            changed
        }
        _ => false,
    }
}

/// Content → Markdown. Blocks are separated by blank lines; the result ends
/// without a trailing newline.
pub fn serialize_markdown(fragment: &Fragment) -> String {
    let blocks: Vec<Vec<String>> = fragment.iter().filter_map(block).collect();
    join_blocks(blocks).join("\n")
}

/// Joins blocks with a blank line between them.
fn join_blocks(blocks: Vec<Vec<String>>) -> Vec<String> {
    let mut lines = Vec::new();
    for (index, block) in blocks.into_iter().enumerate() {
        if index > 0 {
            lines.push(String::new());
        }
        lines.extend(block);
    }
    lines
}

/// Prefixes every line: `first` on the first, `rest` on the others (blank
/// lines stay blank unless `rest` has visible characters, as in quotes).
fn prefix(lines: Vec<String>, first: &str, rest: &str) -> Vec<String> {
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let marker = if index == 0 { first } else { rest };
            if line.is_empty() {
                marker.trim_end().to_string()
            } else {
                format!("{marker}{line}")
            }
        })
        .collect()
}

fn str_attr<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    node.attr(name).and_then(Value::as_str)
}

/// The lines of a block node, or `None` for nothing to write.
fn block(node: &Node) -> Option<Vec<String>> {
    Some(match node.type_name() {
        "paragraph" => {
            if node.content().size() == 0 {
                return None;
            }
            inline_lines(node, false)
        }
        "heading" => {
            let level = node
                .attr("level")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 6) as usize;
            // ATX headings are one line: breaks become HTML.
            let mut text = inline_lines(node, true).join("");
            // Trailing `#`s would read as the optional closing sequence.
            let run = text.len() - text.trim_end_matches('#').len();
            if run > 0 && !escaped_before(&text, text.len() - run) {
                text.insert(text.len() - run, '\\');
            }
            let hashes = "#".repeat(level);
            vec![if text.is_empty() {
                hashes
            } else {
                format!("{hashes} {text}")
            }]
        }
        "blockquote" => {
            let inner = join_blocks(node.content().iter().filter_map(block).collect());
            if inner.is_empty() {
                vec![">".into()]
            } else {
                prefix(inner, "> ", "> ")
            }
        }
        "bulletList" => list(node, |_| "- ".into()),
        "orderedList" => {
            let start = node.attr("start").and_then(Value::as_u64).unwrap_or(1);
            list(node, |index| format!("{}. ", start + index as u64))
        }
        "taskList" => list(node, |_| "- ".into()),
        "codeBlock" => {
            let code: String = node.content().iter().filter_map(Node::text).collect();
            let fence = "`".repeat(longest_run(&code, '`').max(2) + 1);
            let language = str_attr(node, "language").unwrap_or("");
            let mut lines = vec![format!("{fence}{language}")];
            lines.extend(code.split('\n').map(str::to_string));
            lines.push(fence);
            lines
        }
        "horizontalRule" => vec!["---".into()],
        "image" => vec![image(node)],
        _ if node.is_textblock() => inline_lines(node, false),
        _ if node.is_leaf() => return None,
        _ => {
            let inner = join_blocks(node.content().iter().filter_map(block).collect());
            if inner.is_empty() {
                return None;
            }
            inner
        }
    })
}

/// A list; `marker` gives each item's marker from its index.
fn list(node: &Node, marker: impl Fn(usize) -> String) -> Vec<String> {
    let mut lines = Vec::new();
    for (index, item) in node.content().iter().enumerate() {
        let mut first = marker(index);
        if item.type_name() == "taskItem" {
            let checked = item
                .attr("checked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            first.push_str(if checked { "[x] " } else { "[ ] " });
        }
        // Continuation lines line up with the text after the list marker.
        let indent = " ".repeat(marker(index).len());
        let mut content = Vec::new();
        for (child_index, child) in item.content().iter().enumerate() {
            let Some(child_lines) = block(child) else {
                continue;
            };
            // A nested list follows its paragraph directly (a tight list);
            // other blocks need a blank line.
            if child_index > 0 && !child.type_name().ends_with("List") {
                content.push(String::new());
            }
            content.extend(child_lines);
        }
        if content.is_empty() {
            content.push(String::new());
        }
        let mut item_lines = prefix(content, &first, &indent);
        if let Some(line) = item_lines.first_mut()
            && line.is_empty()
        {
            *line = first.trim_end().to_string();
        }
        lines.extend(item_lines);
    }
    lines
}

fn image(node: &Node) -> String {
    let alt = escape_text(str_attr(node, "alt").unwrap_or(""));
    let src = str_attr(node, "src").unwrap_or("");
    match str_attr(node, "title") {
        Some(title) => format!("![{alt}]({} \"{}\")", url(src), title.replace('"', "\\\"")),
        None => format!("![{alt}]({})", url(src)),
    }
}

/// A link destination, in angle brackets when it has spaces or parens.
fn url(href: &str) -> String {
    if href.contains([' ', '(', ')', '<', '>']) {
        format!("<{}>", href.replace('<', "%3C").replace('>', "%3E"))
    } else {
        href.to_string()
    }
}

fn longest_run(text: &str, ch: char) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for c in text.chars() {
        if c == ch {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

/// The syntax around a mark: opening and closing. `html` asks for tags
/// instead of delimiters, for runs where delimiters would not be read as
/// such (see `delimiters_fit`).
fn delimiters(mark: &Mark, html: bool) -> (String, String) {
    match mark.mark_type().name() {
        "bold" if html => ("<strong>".into(), "</strong>".into()),
        "italic" if html => ("<em>".into(), "</em>".into()),
        "strike" if html => ("<s>".into(), "</s>".into()),
        "bold" => ("**".into(), "**".into()),
        "italic" => ("*".into(), "*".into()),
        "strike" => ("~~".into(), "~~".into()),
        "underline" => ("<u>".into(), "</u>".into()),
        "highlight" => ("<mark>".into(), "</mark>".into()),
        "subscript" => ("<sub>".into(), "</sub>".into()),
        "superscript" => ("<sup>".into(), "</sup>".into()),
        "link" => {
            let href = mark.attr("href").and_then(Value::as_str).unwrap_or("");
            let close = match mark.attr("title").and_then(Value::as_str) {
                Some(title) => format!("]({} \"{}\")", url(href), title.replace('"', "\\\"")),
                None => format!("]({})", url(href)),
            };
            ("[".into(), close)
        }
        _ => (String::new(), String::new()),
    }
}

/// A textblock's inline content as lines (hard breaks end lines, written
/// as a backslash). `single_line` writes breaks as `<br>` instead.
fn inline_lines(node: &Node, single_line: bool) -> Vec<String> {
    let mut out = String::new();
    // Open marks with their closing syntax, outermost first.
    let mut open: Vec<(Mark, String)> = Vec::new();
    // Where the last closing syntax ended, so that an opening right after
    // it does not merge into one delimiter run (`*a***b**`).
    let mut closed_at = usize::MAX;
    // Where a code span ending in a space ended: HTML collapses a space
    // right after it as well.
    let mut code_space_at = usize::MAX;
    fn close_to(
        out: &mut String,
        open: &mut Vec<(Mark, String)>,
        keep: usize,
        closed_at: &mut usize,
    ) {
        if open.len() > keep {
            while open.len() > keep {
                let (_, close) = open.pop().expect("checked length");
                out.push_str(&close);
            }
            *closed_at = out.len();
        }
    }
    let children: Vec<&Node> = node.content().iter().collect();
    let count = children.len();
    for (index, child) in children.iter().copied().enumerate() {
        let Some(text) = child.text() else {
            close_to(&mut out, &mut open, 0, &mut closed_at);
            match child.type_name() {
                "hardBreak" if single_line => out.push_str("<br>"),
                "hardBreak" => out.push_str("\\\n"),
                "image" => out.push_str(&image(child)),
                _ => {}
            }
            continue;
        };
        // Links outermost; code is not a delimiter pair but a code span.
        let mut marks: Vec<Mark> = child
            .marks()
            .iter()
            .filter(|m| m.mark_type().name() != "code")
            .cloned()
            .collect();
        marks.sort_by_key(|m| m.mark_type().name() != "link");
        let code = child.marks().iter().any(|m| m.mark_type().name() == "code");
        let keep = open
            .iter()
            .zip(&marks)
            .take_while(|((a, _), b)| a == *b)
            .count();
        close_to(&mut out, &mut open, keep, &mut closed_at);
        // Markdown wants delimiters next to non-space characters, so spaces
        // at the edges of marked text go outside them.
        // Code spans keep their spaces inside the backticks.
        let (leading, core, trailing) = if code && !text.trim_matches(' ').is_empty() {
            ("", text, "")
        } else {
            (
                &text[..text.len() - text.trim_start_matches(' ').len()],
                text.trim_matches(' '),
                &text[text.trim_end_matches(' ').len()..],
            )
        };
        let last = index + 1 == count;
        if core.is_empty() {
            close_to(&mut out, &mut open, 0, &mut closed_at);
            push_text(&mut out, text, last, code_space_at);
            continue;
        }
        push_text(&mut out, leading, false, code_space_at);
        let opened = open.len();
        for (k, mark) in marks.iter().enumerate().skip(opened) {
            // Marks outside this one: already open, or opened just before.
            let outer: Vec<&Mark> = marks[..k].iter().collect();
            let inner_opens = k + 1 < marks.len() || code;
            let merges = closed_at == out.len()
                && delimiters(mark, false)
                    .0
                    .chars()
                    .next()
                    .is_some_and(|c| out.ends_with(c));
            let html = merges || !delimiters_fit(&out, &children, index, mark, &outer, inner_opens);
            let (start, end) = delimiters(mark, html);
            // `![` would start an image.
            if start == "[" && out.ends_with('!') && !escaped_before(&out, out.len() - 1) {
                out.insert(out.len() - 1, '\\');
            }
            out.push_str(&start);
            open.push((mark.clone(), end));
        }
        if code {
            let ticks = "`".repeat(longest_run(core, '`') + 1);
            // CommonMark strips one space from each side when both have
            // one, and needs a space next to backticks inside.
            let pad = if core.starts_with('`')
                || core.ends_with('`')
                || (core.starts_with(' ') && core.ends_with(' '))
            {
                " "
            } else {
                ""
            };
            // Inline code collapses repeated spaces like other HTML text;
            // literal no-break spaces (read back as spaces) keep them.
            let mut kept = String::with_capacity(core.len());
            let mut space = ends_in_space(&out, code_space_at);
            for ch in core.chars() {
                kept.push(if ch == ' ' && space { '\u{a0}' } else { ch });
                space = ch == ' ';
            }
            out.push_str(&format!("{ticks}{pad}{kept}{pad}{ticks}"));
            if core.ends_with(' ') {
                code_space_at = out.len();
            }
        } else {
            push_text(&mut out, core, last && trailing.is_empty(), code_space_at);
        }
        if !trailing.is_empty() {
            close_to(&mut out, &mut open, 0, &mut closed_at);
            push_text(&mut out, trailing, last, code_space_at);
        }
    }
    close_to(&mut out, &mut open, 0, &mut closed_at);
    out.split('\n').map(str::to_string).collect()
}

/// Whether the character at byte `at` is backslash-escaped (an odd number
/// of backslashes before it).
fn escaped_before(out: &str, at: usize) -> bool {
    out[..at].chars().rev().take_while(|&c| c == '\\').count() % 2 == 1
}

/// Whether what was written so far ends in a space HTML would collapse a
/// following one into: plain text ending in a space, or a code span whose
/// content does (it ended at `code_space_at`).
fn ends_in_space(out: &str, code_space_at: usize) -> bool {
    out.ends_with(' ') || out.len() == code_space_at
}

fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_whitespace())
}

/// Whether `*` / `~~` delimiters around the run of `mark` starting at
/// `children[index]` would be read as such: CommonMark does not open a
/// delimiter run before punctuation that follows a word character, nor
/// close one after punctuation that precedes one. Syntax of marks nested
/// inside (`inner_opens` at the start; at the end, marks of the last node
/// other than `outer` ones) counts as punctuation.
fn delimiters_fit(
    out: &str,
    children: &[&Node],
    index: usize,
    mark: &Mark,
    outer: &[&Mark],
    inner_opens: bool,
) -> bool {
    let mut text = String::new();
    let mut end = index;
    while let Some(child) = children.get(end)
        && let Some(t) = child.text()
        && child.marks().contains(mark)
    {
        text.push_str(t);
        end += 1;
    }
    let core = text.trim_matches(' ');
    let (Some(first), Some(last)) = (core.chars().next(), core.chars().next_back()) else {
        return true;
    };
    let before = out.chars().next_back();
    let after = if text.ends_with(' ') {
        Some(' ')
    } else {
        children
            .get(end)
            .and_then(|next| next.text())
            .and_then(|t| t.chars().next())
    };
    let inner_closes = children[end - 1]
        .marks()
        .iter()
        .any(|m| m != mark && !outer.contains(&m));
    let word = |c: Option<char>| c.is_some_and(|c| !c.is_whitespace() && !is_punctuation(c));
    let starts_with_punctuation = inner_opens || is_punctuation(first);
    let ends_with_punctuation = inner_closes || is_punctuation(last);
    !(starts_with_punctuation && word(before)) && !(ends_with_punctuation && word(after))
}

/// Writes plain text, escaping Markdown syntax, keeping spaces Markdown
/// would drop (at the start or end of a line, or doubled) as `&nbsp;`, and
/// escaping what would start a block at the beginning of a line.
fn push_text(out: &mut String, text: &str, at_end: bool, code_space_at: usize) {
    let line_start = out.is_empty() || out.ends_with('\n');
    let start = out.len();
    let chars: Vec<char> = escape_text(text).chars().collect();
    let mut previous_space = line_start || ends_in_space(out, code_space_at);
    for (index, &ch) in chars.iter().enumerate() {
        if ch == ' ' {
            if previous_space || (at_end && index + 1 == chars.len()) {
                out.push_str("&nbsp;");
                previous_space = false;
            } else {
                out.push(' ');
                previous_space = true;
            }
        } else {
            out.push(ch);
            previous_space = false;
        }
    }
    if line_start && out.len() > start {
        escape_line_start(out, start);
    }
}

/// Backslash-escapes characters with Markdown meaning inside text.
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    for (index, &ch) in chars.iter().enumerate() {
        match ch {
            '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>' | '~' | '|' => {
                out.push('\\');
                out.push(ch);
            }
            // Only where it would start an entity (`&amp;`).
            '&' if chars[index + 1..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == '#')
                .count()
                > 0
                && chars[index + 1..]
                    .iter()
                    .find(|c| !(c.is_ascii_alphanumeric() || **c == '#'))
                    == Some(&';') =>
            {
                out.push_str("\\&");
            }
            '\n' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

/// Escapes what would start a block at the beginning of the line starting
/// at `line_start`: `#`, `-`, `+`, `=`, and `1.` / `1)`.
fn escape_line_start(out: &mut String, line_start: usize) {
    let line = &out[line_start..];
    let first = line.chars().next();
    let needs = match first {
        Some('#' | '-' | '+' | '=') => true,
        Some(c) if c.is_ascii_digit() => {
            let digits = line.chars().take_while(char::is_ascii_digit).count();
            matches!(line[digits..].chars().next(), Some('.' | ')'))
        }
        _ => false,
    };
    if !needs {
        return;
    }
    match first {
        Some(c) if c.is_ascii_digit() => {
            let digits = line.chars().take_while(char::is_ascii_digit).count();
            out.insert(line_start + digits, '\\');
        }
        _ => out.insert(line_start, '\\'),
    }
}
