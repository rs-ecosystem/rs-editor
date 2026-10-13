//! Document → HTML in Tiptap's markup, so the result pastes well into other
//! editors and round-trips through [`crate::parse_slice`].

use rs_editor_model::{Fragment, Mark, Node, Slice, fragment_to_json};
use serde_json::{Value, json};

fn escape(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

fn attr(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    escape(value, out);
    out.push('"');
}

fn str_attr<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    node.attr(name).and_then(Value::as_str)
}

fn align(out: &mut String, node: &Node) {
    if let Some(align) = str_attr(node, "textAlign").filter(|a| *a != "left") {
        attr(out, "style", &format!("text-align: {align}"));
    }
}

/// Opening and closing tags of a mark.
fn mark_tags(mark: &Mark) -> (String, &'static str) {
    let mut open = String::new();
    let close = match mark.mark_type().name() {
        "bold" => {
            open.push_str("<strong>");
            "</strong>"
        }
        "italic" => {
            open.push_str("<em>");
            "</em>"
        }
        "underline" => {
            open.push_str("<u>");
            "</u>"
        }
        "strike" => {
            open.push_str("<s>");
            "</s>"
        }
        "code" => {
            open.push_str("<code>");
            "</code>"
        }
        "subscript" => {
            open.push_str("<sub>");
            "</sub>"
        }
        "superscript" => {
            open.push_str("<sup>");
            "</sup>"
        }
        "highlight" => {
            open.push_str("<mark");
            if let Some(color) = mark.attr("color").and_then(Value::as_str) {
                attr(&mut open, "data-color", color);
                attr(
                    &mut open,
                    "style",
                    &format!("background-color: {color}; color: inherit"),
                );
            }
            open.push('>');
            "</mark>"
        }
        "link" => {
            open.push_str("<a");
            for name in ["href", "target", "rel", "class"] {
                if let Some(value) = mark.attr(name).and_then(Value::as_str) {
                    attr(&mut open, name, value);
                }
            }
            open.push('>');
            "</a>"
        }
        _ => {
            open.push_str("<span>");
            "</span>"
        }
    };
    (open, close)
}

/// Writes text, turning spaces HTML would collapse into no-break spaces.
fn text(out: &mut String, text: &str, at_start: bool, at_end: bool) {
    let chars: Vec<char> = text.chars().collect();
    // Whether the previous character was a space HTML would collapse into.
    let mut collapsible = false;
    for (index, &ch) in chars.iter().enumerate() {
        if ch == ' ' {
            let first = index == 0 && at_start;
            let last = index + 1 == chars.len() && at_end;
            if collapsible || first || last {
                out.push_str("&nbsp;");
                collapsible = false;
            } else {
                out.push(' ');
                collapsible = true;
            }
        } else {
            let mut buffer = [0u8; 4];
            escape(ch.encode_utf8(&mut buffer), out);
            collapsible = false;
        }
    }
}

/// Inline content, nesting marks so that shared marks stay open.
fn inline(out: &mut String, content: &Fragment) {
    let mut open: Vec<(Mark, &'static str)> = Vec::new();
    let count = content.child_count();
    for (index, child) in content.iter().enumerate() {
        let marks: Vec<Mark> = child.marks().iter().cloned().collect();
        let keep = open
            .iter()
            .zip(&marks)
            .take_while(|((a, _), b)| a == *b)
            .count();
        while open.len() > keep {
            let (_, close) = open.pop().expect("checked length");
            out.push_str(close);
        }
        for mark in &marks[keep..] {
            let (tag, close) = mark_tags(mark);
            out.push_str(&tag);
            open.push((mark.clone(), close));
        }
        if let Some(t) = child.text() {
            text(out, t, index == 0, index + 1 == count);
        } else {
            node(out, child);
        }
    }
    while let Some((_, close)) = open.pop() {
        out.push_str(close);
    }
}

fn children(out: &mut String, node: &Node) {
    if node.is_textblock() {
        inline(out, node.content());
    } else {
        for child in node.content().iter() {
            self::node(out, child);
        }
    }
}

fn wrap(out: &mut String, tag: &str, node: &Node, attrs: impl FnOnce(&mut String)) {
    out.push('<');
    out.push_str(tag);
    attrs(out);
    out.push('>');
    children(out, node);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn node(out: &mut String, node: &Node) {
    match node.type_name() {
        "paragraph" => wrap(out, "p", node, |o| align(o, node)),
        "heading" => {
            let level = node
                .attr("level")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 6);
            wrap(out, &format!("h{level}"), node, |o| align(o, node));
        }
        "blockquote" => wrap(out, "blockquote", node, |_| {}),
        "bulletList" => wrap(out, "ul", node, |_| {}),
        "orderedList" => {
            let start = node.attr("start").and_then(Value::as_u64).unwrap_or(1);
            wrap(out, "ol", node, |o| {
                if start != 1 {
                    attr(o, "start", &start.to_string());
                }
            });
        }
        "listItem" => wrap(out, "li", node, |_| {}),
        "taskList" => wrap(out, "ul", node, |o| attr(o, "data-type", "taskList")),
        "taskItem" => {
            let checked = node
                .attr("checked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            out.push_str("<li data-type=\"taskItem\"");
            attr(out, "data-checked", if checked { "true" } else { "false" });
            out.push_str("><label><input type=\"checkbox\"");
            if checked {
                out.push_str(" checked=\"checked\"");
            }
            out.push_str("><span></span></label><div>");
            children(out, node);
            out.push_str("</div></li>");
        }
        "codeBlock" => {
            out.push_str("<pre><code");
            if let Some(language) = str_attr(node, "language") {
                attr(out, "class", &format!("language-{language}"));
            }
            out.push('>');
            for child in node.content().iter() {
                escape(child.text().unwrap_or(""), out);
            }
            out.push_str("</code></pre>");
        }
        "horizontalRule" => out.push_str("<hr>"),
        "hardBreak" => out.push_str("<br>"),
        "image" => {
            out.push_str("<img");
            for name in ["src", "alt", "title"] {
                if let Some(value) = str_attr(node, name) {
                    attr(out, name, value);
                }
            }
            for name in ["width", "height"] {
                if let Some(value) = node.attr(name).and_then(Value::as_f64) {
                    attr(out, name, &value.to_string());
                }
            }
            out.push('>');
        }
        _ if node.is_textblock() => wrap(out, "p", node, |_| {}),
        _ if node.is_leaf() => {}
        _ => children(out, node),
    }
}

/// HTML for a fragment of block (or inline) content.
pub fn serialize_fragment(fragment: &Fragment) -> String {
    let mut out = String::new();
    let inline_only = fragment.iter().all(Node::is_inline);
    if inline_only && fragment.child_count() > 0 {
        inline(&mut out, fragment);
    } else {
        for child in fragment.iter() {
            node(&mut out, child);
        }
    }
    out
}

/// Clipboard HTML for a slice. The wrapper carries the slice as JSON, so
/// pasting back into Rs Editor restores it exactly; other apps see plain
/// Tiptap-style markup.
pub fn serialize_slice(slice: &Slice) -> String {
    let data = json!({
        "content": fragment_to_json(&slice.content),
        "openStart": slice.open_start,
        "openEnd": slice.open_end,
    });
    let mut out = String::from("<div");
    attr(&mut out, "data-rs-editor-slice", &data.to_string());
    out.push('>');
    out.push_str(&serialize_fragment(&slice.content));
    out.push_str("</div>");
    out
}
