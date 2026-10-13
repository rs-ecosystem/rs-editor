//! A small, forgiving HTML tree builder on top of html5gum's tokenizer.
//!
//! Clipboard HTML from browsers, Google Docs and Word is mostly well formed;
//! this handles the common omissions (unclosed `<p>`, `<li>`, table cells),
//! void elements, and drops content that is never text (scripts, styles,
//! the document head).

use html5gum::{DefaultEmitter, Token, Tokenizer};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Dom {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Dom>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.attr("class")
            .is_some_and(|classes| classes.split_whitespace().any(|c| c == class))
    }

    /// The first descendant element matching `pred`.
    pub fn find(&self, pred: &dyn Fn(&Element) -> bool) -> Option<&Element> {
        for child in &self.children {
            if let Dom::Element(element) = child {
                if pred(element) {
                    return Some(element);
                }
                if let Some(found) = element.find(pred) {
                    return Some(found);
                }
            }
        }
        None
    }
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// Elements whose content is never document text.
const SKIPPED: &[&str] = &[
    "head", "script", "style", "title", "template", "noscript", "iframe", "xmp", "textarea",
    "object", "svg", "math",
];

/// Starting one of these closes an open `<p>`.
const CLOSES_P: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "details",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

/// `<p>` never closes across these.
const P_SCOPE: &[&str] = &["td", "th", "table", "button", "li", "blockquote", "div"];

/// Removes the Windows `CF_HTML` header and keeps the copied fragment.
fn fragment_of(html: &str) -> &str {
    const START: &str = "<!--StartFragment-->";
    const END: &str = "<!--EndFragment-->";
    match (html.find(START), html.find(END)) {
        (Some(start), Some(end)) if start + START.len() <= end => &html[start + START.len()..end],
        _ => html,
    }
}

fn lower(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_ascii_lowercase()
}

/// Parses `html` into a root element named `#root`.
pub(crate) fn parse(html: &str) -> Element {
    let mut emitter = DefaultEmitter::default();
    emitter.naively_switch_states(true);
    let tokenizer = Tokenizer::new_with_emitter(fragment_of(html), emitter);
    let mut stack = vec![Element {
        name: "#root".into(),
        ..Default::default()
    }];
    // Depth of skipped elements we are inside.
    let mut skipping = 0usize;

    fn close_to(stack: &mut Vec<Element>, index: usize) {
        while stack.len() > index.max(1) {
            let element = stack.pop().expect("non-empty stack");
            if let Some(parent) = stack.last_mut() {
                parent.children.push(Dom::Element(element));
            }
        }
    }

    for token in tokenizer.flatten() {
        match token {
            Token::StartTag(tag) => {
                let name = lower(&tag.name);
                if skipping > 0 {
                    if SKIPPED.contains(&name.as_str()) && !tag.self_closing {
                        skipping += 1;
                    }
                    continue;
                }
                if SKIPPED.contains(&name.as_str()) {
                    if !tag.self_closing && !VOID.contains(&name.as_str()) {
                        skipping = 1;
                    }
                    continue;
                }
                // Implied end tags.
                if CLOSES_P.contains(&name.as_str())
                    && let Some(index) = stack.iter().rposition(|e| e.name == "p")
                    && !stack[index..]
                        .iter()
                        .skip(1)
                        .any(|e| P_SCOPE.contains(&e.name.as_str()))
                {
                    close_to(&mut stack, index);
                }
                let closes_same = |stack: &Vec<Element>, names: &[&str], boundary: &[&str]| {
                    stack
                        .iter()
                        .rposition(|e| {
                            names.contains(&e.name.as_str()) || boundary.contains(&e.name.as_str())
                        })
                        .filter(|&i| names.contains(&stack[i].name.as_str()))
                };
                let implied = match name.as_str() {
                    "li" => closes_same(&stack, &["li"], &["ul", "ol"]),
                    "dt" | "dd" => closes_same(&stack, &["dt", "dd"], &["dl"]),
                    "tr" => closes_same(&stack, &["tr"], &["table", "tbody", "thead", "tfoot"]),
                    "td" | "th" => closes_same(&stack, &["td", "th"], &["tr", "table"]),
                    "option" => closes_same(&stack, &["option"], &["select"]),
                    _ => None,
                };
                if let Some(index) = implied {
                    close_to(&mut stack, index);
                }
                let attrs = tag
                    .attributes
                    .iter()
                    .map(|(key, value)| (lower(key), String::from_utf8_lossy(value).into_owned()))
                    .collect();
                let element = Element {
                    name: name.clone(),
                    attrs,
                    children: Vec::new(),
                };
                if VOID.contains(&name.as_str()) || tag.self_closing {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(Dom::Element(element));
                    }
                } else {
                    stack.push(element);
                }
            }
            Token::EndTag(tag) => {
                let name = lower(&tag.name);
                if skipping > 0 {
                    if SKIPPED.contains(&name.as_str()) {
                        skipping -= 1;
                    }
                    continue;
                }
                if let Some(index) = stack.iter().rposition(|e| e.name == name)
                    && index > 0
                {
                    close_to(&mut stack, index);
                }
            }
            Token::String(text) => {
                if skipping > 0 {
                    continue;
                }
                let text = String::from_utf8_lossy(&text.value).into_owned();
                if let Some(parent) = stack.last_mut() {
                    if let Some(Dom::Text(last)) = parent.children.last_mut() {
                        last.push_str(&text);
                    } else {
                        parent.children.push(Dom::Text(text));
                    }
                }
            }
            Token::Comment(_) | Token::Doctype(_) | Token::Error(_) => {}
        }
    }
    close_to(&mut stack, 1);
    stack.pop().unwrap_or_default()
}
