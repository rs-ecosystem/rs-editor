//! HTML → document, the way ProseMirror's DOMParser does it: each element
//! maps to a node or a mark, and nodes are placed wherever the schema
//! allows, adding wrappers (a paragraph around loose text, a list around a
//! stray item) and closing open nodes as needed. Any HTML gives a valid
//! document.

use rs_editor_model::{ContentMatch, Fragment, Mark, MarkSet, Node, NodeType, Schema, Slice};
use serde_json::{Map, Value, json};

use crate::dom::{Dom, Element};

/// Block-level elements without a node of their own: they separate
/// paragraphs but add no structure.
const BLOCK_BOUNDARIES: &[&str] = &[
    "address",
    "article",
    "aside",
    "caption",
    "center",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "header",
    "hgroup",
    "main",
    "nav",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
];

struct Frame {
    id: u64,
    ty: NodeType,
    attrs: Map<String, Value>,
    content: Vec<Node>,
    matched: ContentMatch,
    /// Opened for an element (not as an implied wrapper).
    solid: bool,
    /// Keep whitespace as is (code blocks).
    preserve: bool,
}

struct Parser<'a> {
    schema: &'a Schema,
    stack: Vec<Frame>,
    marks: Vec<Mark>,
    next_id: u64,
}

/// Allowed link targets: no script or data URLs from pasted content.
fn safe_href(href: &str) -> bool {
    let lower = href.trim().to_ascii_lowercase();
    !(lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("data:"))
}

fn safe_src(src: &str) -> bool {
    let lower = src.trim().to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:image/")
        || lower.starts_with("blob:")
}

/// `style` declarations as (property, value) pairs, lowercased.
fn styles(element: &Element) -> Vec<(String, String)> {
    element
        .attr("style")
        .unwrap_or("")
        .split(';')
        .filter_map(|decl| {
            let (key, value) = decl.split_once(':')?;
            Some((
                key.trim().to_ascii_lowercase(),
                value.trim().to_ascii_lowercase(),
            ))
        })
        .collect()
}

fn style<'s>(styles: &'s [(String, String)], key: &str) -> Option<&'s str> {
    styles
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

impl<'a> Parser<'a> {
    fn new(schema: &'a Schema) -> Self {
        let top = schema.top_node_type();
        Self {
            schema,
            stack: vec![Frame {
                id: 0,
                matched: top.content_match(),
                ty: top,
                attrs: Map::new(),
                content: Vec::new(),
                solid: true,
                preserve: false,
            }],
            marks: Vec::new(),
            next_id: 1,
        }
    }

    fn node_type(&self, name: &str) -> Option<NodeType> {
        self.schema.node_type(name).ok()
    }

    fn top(&mut self) -> &mut Frame {
        self.stack.last_mut().expect("the root frame stays")
    }

    /// Closes frames and opens wrappers so the top frame accepts `ty`.
    fn find_place(&mut self, ty: &NodeType) -> bool {
        for depth in (0..self.stack.len()).rev() {
            if let Some(wrappers) = self.stack[depth].matched.find_wrapping(ty) {
                while self.stack.len() > depth + 1 {
                    self.close_top();
                }
                for wrapper in wrappers {
                    self.push(wrapper, Map::new(), false);
                }
                return true;
            }
        }
        false
    }

    fn push(&mut self, ty: NodeType, attrs: Map<String, Value>, solid: bool) {
        let preserve = ty.spec().code || self.stack.last().is_some_and(|f| f.preserve);
        // The parent's content moves past the child as soon as it opens, so
        // siblings that may only follow it (a list after a paragraph) fit.
        if let Some(parent) = self.stack.last_mut()
            && let Some(next) = parent.matched.match_type(&ty)
        {
            parent.matched = next;
        }
        self.next_id += 1;
        self.stack.push(Frame {
            id: self.next_id,
            matched: ty.content_match(),
            ty,
            attrs,
            content: Vec::new(),
            solid,
            preserve,
        });
    }

    /// Opens a node for an element's content.
    fn open(&mut self, ty: NodeType, attrs: Map<String, Value>) -> bool {
        if !self.find_place(&ty) {
            return false;
        }
        self.push(ty, attrs, true);
        true
    }

    fn close_top(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let mut frame = self.stack.pop().expect("checked length");
        if frame.ty.is_textblock() && !frame.preserve {
            // Trailing spaces end a block in HTML rendering.
            if let Some(last) = frame.content.last()
                && let Some(text) = last.text()
            {
                let trimmed = text.trim_end_matches(' ');
                if trimmed.is_empty() {
                    frame.content.pop();
                } else if trimmed.len() != text.len() {
                    let node = last.with_text(trimmed.to_owned());
                    *frame.content.last_mut().expect("just read") = node;
                }
            }
        }
        // Implied paragraphs with nothing in them (whitespace between
        // blocks) disappear.
        if !frame.solid && frame.ty.is_textblock() && frame.content.is_empty() {
            return;
        }
        let content = Fragment::from_array(frame.content);
        // Attributes the schema rejects fall back to the defaults.
        let Some(node) = frame
            .ty
            .create_and_fill(Some(&frame.attrs), content.clone(), MarkSet::none())
            .or_else(|| frame.ty.create_and_fill(None, content, MarkSet::none()))
        else {
            return;
        };
        // Its place in the parent was taken when it opened.
        self.append(node);
    }

    /// Adds a node to the top frame (which must accept it).
    fn add(&mut self, node: Node) {
        let top = self.top();
        if let Some(next) = top.matched.match_type(node.node_type()) {
            top.matched = next;
        }
        self.append(node);
    }

    fn append(&mut self, node: Node) {
        let top = self.top();
        if let (Some(last), Some(text)) = (top.content.last(), node.text())
            && let Some(previous) = last.text()
            && last.marks() == node.marks()
        {
            let merged = last.with_text(format!("{previous}{text}"));
            *top.content.last_mut().expect("just read") = merged;
            return;
        }
        top.content.push(node);
    }

    fn insert(&mut self, node: Node) {
        if self.find_place(node.node_type()) {
            self.add(node);
        }
    }

    /// Ends implied paragraphs, at the edges of block-level elements.
    fn end_textblock(&mut self) {
        while self.stack.len() > 1 {
            let top = self.stack.last().expect("checked length");
            if top.solid {
                break;
            }
            self.close_top();
        }
    }

    fn add_text(&mut self, raw: &str) {
        let preserve = self.stack.last().is_some_and(|f| f.preserve);
        let mut text = if preserve {
            raw.replace("\r\n", "\n")
        } else {
            let mut collapsed = String::with_capacity(raw.len());
            let mut space = false;
            for ch in raw.chars() {
                if matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                    if !space {
                        collapsed.push(' ');
                    }
                    space = true;
                } else {
                    collapsed.push(ch);
                    space = false;
                }
            }
            collapsed
        };
        if text.is_empty() {
            return;
        }
        if !preserve {
            let in_textblock = self.stack.last().is_some_and(|f| f.ty.is_textblock());
            if text == " " && !in_textblock {
                // Whitespace between blocks.
                return;
            }
            // Spaces at a block start, or after a space, collapse away.
            let previous_space = !in_textblock
                || self.stack.last().is_some_and(|f| {
                    f.content
                        .last()
                        .is_none_or(|n| n.text().is_none_or(|t| t.ends_with(' ')))
                });
            if previous_space {
                text = text.trim_start_matches(' ').to_owned();
            }
            if text.is_empty() {
                return;
            }
        }
        let Ok(text_type) = self.schema.node_type("text") else {
            return;
        };
        if !self.find_place(&text_type) {
            return;
        }
        let parent = self.stack.last().expect("root frame").ty.clone();
        let mut set = MarkSet::none();
        for mark in &self.marks {
            set = mark.add_to_set(&set);
        }
        let set = parent.allowed_marks(&set);
        if let Ok(node) = self.schema.text(text, set) {
            self.add(node);
        }
    }

    fn mark(&self, name: &str, attrs: Option<Map<String, Value>>) -> Option<Mark> {
        self.schema
            .mark_type(name)
            .ok()?
            .create(attrs.as_ref())
            .ok()
    }

    /// Marks an element applies to its content (tags and inline styles).
    fn element_marks(&self, element: &Element) -> (Vec<Mark>, Vec<&'static str>) {
        let mut add = Vec::new();
        let mut remove = Vec::new();
        let styles = styles(element);
        let weight = style(&styles, "font-weight");
        let not_bold = weight.is_some_and(|w| {
            matches!(
                w,
                "normal" | "lighter" | "100" | "200" | "300" | "400" | "500"
            )
        });
        let mut push = |name: &str, attrs: Option<Map<String, Value>>| {
            if let Some(mark) = self.mark(name, attrs) {
                add.push(mark);
            }
        };
        match element.name.as_str() {
            // Google Docs wraps everything in <b style="font-weight:normal">.
            "strong" | "b" if !not_bold => push("bold", None),
            "em" | "i" | "cite" | "dfn" => push("italic", None),
            "u" | "ins" => push("underline", None),
            "s" | "strike" | "del" => push("strike", None),
            "code" | "kbd" | "samp" | "tt" => push("code", None),
            "sub" => push("subscript", None),
            "sup" => push("superscript", None),
            "mark" => {
                let color = element
                    .attr("data-color")
                    .map(str::to_owned)
                    .or_else(|| style(&styles, "background-color").map(str::to_owned));
                let attrs =
                    color.map(|c| json!({"color": c}).as_object().cloned().unwrap_or_default());
                push("highlight", attrs);
            }
            "a" => {
                if let Some(href) = element
                    .attr("href")
                    .filter(|h| safe_href(h) && !h.is_empty())
                {
                    let mut attrs = Map::new();
                    attrs.insert("href".into(), Value::String(href.to_owned()));
                    if let Some(target) = element.attr("target") {
                        attrs.insert("target".into(), Value::String(target.to_owned()));
                    }
                    push("link", Some(attrs));
                }
            }
            _ => {}
        }
        if weight.is_some_and(|w| matches!(w, "bold" | "bolder" | "600" | "700" | "800" | "900"))
            && !matches!(
                element.name.as_str(),
                "strong" | "b" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "th"
            )
        {
            push("bold", None);
        }
        if not_bold {
            remove.push("bold");
        }
        if style(&styles, "font-style")
            .is_some_and(|s| s.contains("italic") || s.contains("oblique"))
        {
            push("italic", None);
        }
        let decoration = style(&styles, "text-decoration")
            .or_else(|| style(&styles, "text-decoration-line"))
            .unwrap_or("");
        if decoration.contains("underline") {
            push("underline", None);
        }
        if decoration.contains("line-through") {
            push("strike", None);
        }
        match style(&styles, "vertical-align") {
            Some("super") => push("superscript", None),
            Some("sub") => push("subscript", None),
            _ => {}
        }
        (add, remove)
    }

    fn text_align(element: &Element) -> Map<String, Value> {
        let mut attrs = Map::new();
        let styles = styles(element);
        if let Some(align) = style(&styles, "text-align").or_else(|| element.attr("align"))
            && matches!(align, "left" | "center" | "right" | "justify")
        {
            attrs.insert("textAlign".into(), Value::String(align.to_owned()));
        }
        attrs
    }

    /// Parses an element's children inside node `name` (if the schema has
    /// it); returns false when it does not.
    fn with_node(&mut self, name: &str, attrs: Map<String, Value>, element: &Element) -> bool {
        let Some(ty) = self.node_type(name) else {
            return false;
        };
        if !self.open(ty, attrs) {
            return false;
        }
        let id = self.stack.last().expect("just opened").id;
        self.children(element);
        self.close_frame(id);
        true
    }

    /// Closes frame `id` and everything opened inside it, unless content
    /// already closed it.
    fn close_frame(&mut self, id: u64) {
        if let Some(index) = self.stack.iter().position(|f| f.id == id) {
            while self.stack.len() > index.max(1) {
                self.close_top();
            }
        }
    }

    fn leaf(&mut self, name: &str, attrs: Map<String, Value>) {
        if let Some(ty) = self.node_type(name)
            && let Ok(node) = ty
                .create(Some(&attrs), Fragment::empty(), MarkSet::none())
                .or_else(|_| ty.create(None, Fragment::empty(), MarkSet::none()))
        {
            self.insert(node);
        }
    }

    fn children(&mut self, element: &Element) {
        for child in &element.children {
            match child {
                Dom::Text(text) => self.add_text(text),
                Dom::Element(child) => self.element(child),
            }
        }
    }

    fn element(&mut self, element: &Element) {
        let name = element.name.as_str();
        // Marks wrap whatever the element contains.
        let (add, remove) = self.element_marks(element);
        let saved = self.marks.clone();
        self.marks
            .retain(|m| !remove.contains(&m.mark_type().name()));
        self.marks.extend(add);

        match name {
            "p" => {
                let attrs = Self::text_align(element);
                if !self.with_node("paragraph", attrs, element) {
                    self.children(element);
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let mut attrs = Self::text_align(element);
                let level = u64::from(name.as_bytes()[1] - b'0');
                attrs.insert("level".into(), json!(level));
                if !self.with_node("heading", attrs, element) {
                    self.with_node("paragraph", Map::new(), element);
                }
            }
            "blockquote" => {
                if !self.with_node("blockquote", Map::new(), element) {
                    self.children(element);
                }
            }
            "pre" => self.code_block(element),
            "ul" => {
                let task = element.attr("data-type") == Some("taskList")
                    || element.has_class("contains-task-list");
                let list = if task { "taskList" } else { "bulletList" };
                if !self.with_node(list, Map::new(), element) {
                    self.children(element);
                }
            }
            "ol" => {
                let mut attrs = Map::new();
                if let Some(start) = element
                    .attr("start")
                    .and_then(|s| s.trim().parse::<u64>().ok())
                {
                    attrs.insert("start".into(), json!(start));
                }
                if !self.with_node("orderedList", attrs, element) {
                    self.children(element);
                }
            }
            "li" => self.list_item(element),
            "hr" => self.leaf("horizontalRule", Map::new()),
            // Chrome and Safari mark the end of a copied block with this.
            "br" if element.has_class("Apple-interchange-newline") => {}
            "br" => {
                if self.stack.last().is_some_and(|f| f.preserve) {
                    self.add_text("\n");
                } else {
                    self.leaf("hardBreak", Map::new());
                }
            }
            "img" => {
                if let Some(src) = element.attr("src").filter(|s| safe_src(s)) {
                    let mut attrs = Map::new();
                    attrs.insert("src".into(), json!(src));
                    for key in ["alt", "title"] {
                        if let Some(value) = element.attr(key) {
                            attrs.insert(key.into(), json!(value));
                        }
                    }
                    for key in ["width", "height"] {
                        if let Some(value) =
                            element.attr(key).and_then(|v| v.trim().parse::<f64>().ok())
                        {
                            attrs.insert(key.into(), json!(value));
                        }
                    }
                    self.leaf("image", attrs);
                }
            }
            // Task list checkboxes become the item's `checked` attribute.
            "input" | "button" | "select" => {}
            _ if BLOCK_BOUNDARIES.contains(&name) => {
                self.end_textblock();
                self.children(element);
                self.end_textblock();
            }
            _ => self.children(element),
        }
        self.marks = saved;
    }

    fn list_item(&mut self, element: &Element) {
        let parent_task = self.stack.last().is_some_and(|f| f.ty.name() == "taskList");
        let checkbox = element.find(&|e| {
            e.name == "input"
                && e.attr("type")
                    .is_some_and(|t| t.eq_ignore_ascii_case("checkbox"))
        });
        let task = parent_task
            || element.attr("data-type") == Some("taskItem")
            || element.has_class("task-list-item");
        if task && self.node_type("taskItem").is_some() {
            let checked = element.attr("data-checked") == Some("true")
                || checkbox.is_some_and(|c| c.attr("checked").is_some());
            let mut attrs = Map::new();
            attrs.insert("checked".into(), json!(checked));
            if !parent_task && let Some(list) = self.node_type("taskList") {
                // A task item outside a task list gets one.
                if self.open(list, Map::new()) {
                    let id = self.stack.last().expect("just opened").id;
                    self.with_node("taskItem", attrs, element);
                    self.close_frame(id);
                    return;
                }
            }
            self.with_node("taskItem", attrs, element);
            return;
        }
        if !self.with_node("listItem", Map::new(), element) {
            self.end_textblock();
            self.children(element);
            self.end_textblock();
        }
    }

    fn code_block(&mut self, element: &Element) {
        let language = element
            .find(&|e| e.name == "code")
            .into_iter()
            .chain(std::iter::once(element))
            .find_map(|e| {
                e.attr("class")?
                    .split_whitespace()
                    .find_map(|c| c.strip_prefix("language-").map(str::to_owned))
            })
            .or_else(|| element.attr("data-language").map(str::to_owned));
        let mut attrs = Map::new();
        if let Some(language) = language {
            attrs.insert("language".into(), json!(language));
        }
        // Marks do not apply inside code.
        let saved = std::mem::take(&mut self.marks);
        if !self.with_node("codeBlock", attrs, element) {
            self.children(element);
        }
        self.marks = saved;
    }

    fn finish(mut self) -> Fragment {
        while self.stack.len() > 1 {
            self.close_top();
        }
        let root = self.stack.pop().expect("root frame");
        Fragment::from_array(root.content)
    }
}

/// Parses HTML into the content of `schema`'s top node.
pub fn parse_fragment(schema: &Schema, html: &str) -> Fragment {
    let root = crate::dom::parse(html);
    let mut parser = Parser::new(schema);
    parser.children(&root);
    parser.finish()
}

/// Parses clipboard HTML into a slice to paste. HTML copied from Rs Editor
/// carries its exact content and is restored as is.
pub fn parse_slice(schema: &Schema, html: &str) -> Slice {
    let root = crate::dom::parse(html);
    if let Some(wrapper) = root.find(&|e| e.attr("data-rs-editor-slice").is_some())
        && let Some(json) = wrapper.attr("data-rs-editor-slice")
        && let Ok(value) = serde_json::from_str::<Value>(json)
        && let Ok(content) = schema.fragment_from_json(value.get("content").unwrap_or(&Value::Null))
    {
        let open = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0) as usize;
        return Slice::new(content, open("openStart"), open("openEnd"));
    }
    let mut parser = Parser::new(schema);
    parser.children(&root);
    Slice::max_open(parser.finish(), false)
}
