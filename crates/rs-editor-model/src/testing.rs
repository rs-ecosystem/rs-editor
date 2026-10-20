//! Test schema and a tagged document builder, like `prosemirror-test-builder`.
//!
//! ```ignore
//! let d = doc(v![p(v!["he<a>llo"]), p(v!["wor<b>ld"])]);
//! d.tag("a"); // position of <a>
//! ```

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::{Map, Value, json};

use crate::fragment::Fragment;
use crate::mark::{Mark, MarkSet};
use crate::node::Node;
use crate::schema::{AttrSpec, MarkSpec, NodeSpec, Schema, SchemaSpec};

/// `prosemirror-schema-basic` plus lists, with ProseMirror's names.
pub fn schema() -> Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA
        .get_or_init(|| Schema::new(schema_spec()).expect("test schema is valid"))
        .clone()
}

/// The spec behind [`schema`], for tests that extend it.
pub fn schema_spec() -> SchemaSpec {
    SchemaSpec::default()
        .node("doc", NodeSpec::new().content("block+"))
        .node(
            "paragraph",
            NodeSpec::new().content("inline*").group("block"),
        )
        .node(
            "blockquote",
            NodeSpec::new().content("block+").group("block").defining(),
        )
        .node("horizontal_rule", NodeSpec::new().group("block"))
        .node(
            "heading",
            NodeSpec::new()
                .attr("level", AttrSpec::with_default(1).validate("number"))
                .content("inline*")
                .group("block")
                .defining(),
        )
        .node(
            "code_block",
            NodeSpec::new()
                .content("text*")
                .marks("")
                .group("block")
                .code()
                .defining(),
        )
        .node("text", NodeSpec::new().group("inline"))
        .node(
            "image",
            NodeSpec::new()
                .inline()
                .attr("src", AttrSpec::required().validate("string"))
                .attr("alt", AttrSpec::with_default(Value::Null))
                .attr("title", AttrSpec::with_default(Value::Null))
                .group("inline")
                .draggable(),
        )
        .node(
            "hard_break",
            NodeSpec::new()
                .inline()
                .group("inline")
                .selectable(false)
                .leaf_text("\n"),
        )
        .node(
            "ordered_list",
            NodeSpec::new()
                .attr("order", AttrSpec::with_default(1))
                .content("list_item+")
                .group("block"),
        )
        .node(
            "bullet_list",
            NodeSpec::new().content("list_item+").group("block"),
        )
        .node(
            "list_item",
            NodeSpec::new().content("paragraph block*").defining(),
        )
        .mark(
            "link",
            MarkSpec::new()
                .attr("href", AttrSpec::required())
                .attr("title", AttrSpec::with_default(Value::Null))
                .inclusive(false),
        )
        .mark("em", MarkSpec::new())
        .mark("strong", MarkSpec::new())
        .mark("code", MarkSpec::new().code())
}

/// One item of a builder call: text (may contain `<tag>`), a tag, a node or a mark.
#[derive(Debug, Clone)]
pub enum Item {
    Text(String),
    Node {
        name: &'static str,
        attrs: Map<String, Value>,
        children: Vec<Item>,
    },
    Mark {
        name: &'static str,
        attrs: Map<String, Value>,
        children: Vec<Item>,
    },
    Tagged(TaggedNode),
}

impl From<&str> for Item {
    fn from(text: &str) -> Self {
        Item::Text(text.to_owned())
    }
}

impl From<TaggedNode> for Item {
    fn from(node: TaggedNode) -> Self {
        Item::Tagged(node)
    }
}

/// Builds a `Vec<Item>` from strings and builder results.
#[macro_export]
macro_rules! v {
    ($($item:expr),* $(,)?) => { vec![$($crate::testing::Item::from($item)),*] };
}

/// A built node plus the positions of its `<tag>` markers.
#[derive(Debug, Clone)]
pub struct TaggedNode {
    pub node: Node,
    pub tags: HashMap<String, usize>,
}

impl TaggedNode {
    /// Position of `<name>`; panics when the tag is missing.
    pub fn tag(&self, name: &str) -> usize {
        *self
            .tags
            .get(name)
            .unwrap_or_else(|| panic!("missing tag <{name}>"))
    }
}

fn split_tags(text: &str) -> Vec<Result<&str, &str>> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let Some(len) = rest[start + 1..].find('>') else {
            break;
        };
        let name = &rest[start + 1..start + 1 + len];
        if name.is_empty() || !name.chars().all(|ch| ch.is_alphanumeric() || ch == '_') {
            break;
        }
        if start > 0 {
            parts.push(Ok(&rest[..start]));
        }
        parts.push(Err(name));
        rest = &rest[start + 2 + len..];
    }
    if !rest.is_empty() {
        parts.push(Ok(rest));
    }
    parts
}

fn build_children(
    schema: &Schema,
    items: &[Item],
    marks: &MarkSet,
    start: usize,
    tags: &mut HashMap<String, usize>,
    out: &mut Vec<Node>,
) -> usize {
    let mut pos = start;
    for item in items {
        match item {
            Item::Text(text) => {
                for part in split_tags(text) {
                    match part {
                        Ok(text) => {
                            let node = schema.text(text, marks.clone()).expect("non-empty text");
                            pos += node.node_size();
                            out.push(node);
                        }
                        Err(tag) => {
                            tags.insert(tag.to_owned(), pos);
                        }
                    }
                }
            }
            Item::Node {
                name,
                attrs,
                children,
            } => {
                let mut inner = Vec::new();
                build_children(
                    schema,
                    children,
                    &MarkSet::none(),
                    pos + 1,
                    tags,
                    &mut inner,
                );
                let ty = schema.node_type(name).expect("known node type");
                let node = ty
                    .create(Some(attrs), Fragment::from_array(inner), marks.clone())
                    .expect("valid attrs");
                pos += node.node_size();
                out.push(node);
            }
            Item::Mark {
                name,
                attrs,
                children,
            } => {
                let mark: Mark = schema
                    .mark_type(name)
                    .expect("known mark type")
                    .create(Some(attrs))
                    .expect("valid attrs");
                pos = build_children(schema, children, &mark.add_to_set(marks), pos, tags, out);
            }
            Item::Tagged(tagged) => {
                for (name, offset) in &tagged.tags {
                    tags.insert(name.clone(), pos + 1 + offset);
                }
                let node = tagged.node.mark(MarkSet::from_marks(
                    marks
                        .iter()
                        .cloned()
                        .chain(tagged.node.marks().iter().cloned()),
                ));
                pos += node.node_size();
                out.push(node);
            }
        }
    }
    pos
}

fn build(name: &'static str, attrs: Map<String, Value>, children: Vec<Item>) -> TaggedNode {
    build_in(&schema(), name, attrs, children)
}

fn build_in(
    schema: &Schema,
    name: &str,
    attrs: Map<String, Value>,
    children: Vec<Item>,
) -> TaggedNode {
    let mut tags = HashMap::new();
    let mut inner = Vec::new();
    build_children(
        schema,
        &children,
        &MarkSet::none(),
        0,
        &mut tags,
        &mut inner,
    );
    let node = schema
        .node_type(name)
        .expect("known node type")
        .create(Some(&attrs), Fragment::from_array(inner), MarkSet::none())
        .expect("valid attrs");
    TaggedNode { node, tags }
}

fn attrs(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

macro_rules! node_builders {
    ($($fn_name:ident => $node:literal $(, $attrs:expr)?;)*) => {$(
        pub fn $fn_name(children: Vec<Item>) -> TaggedNode {
            build($node, attrs(node_builders!(@attrs $($attrs)?)), children)
        }
    )*};
    (@attrs) => { Value::Null };
    (@attrs $attrs:expr) => { $attrs };
}

node_builders! {
    doc => "doc";
    p => "paragraph";
    blockquote => "blockquote";
    h1 => "heading", json!({"level": 1});
    h2 => "heading", json!({"level": 2});
    h3 => "heading", json!({"level": 3});
    pre => "code_block";
    ol => "ordered_list";
    ul => "bullet_list";
    li => "list_item";
}

fn leaf(name: &'static str, attrs_value: Value) -> Item {
    Item::Node {
        name,
        attrs: attrs(attrs_value),
        children: Vec::new(),
    }
}

pub fn hr() -> Item {
    leaf("horizontal_rule", Value::Null)
}

pub fn img() -> Item {
    leaf("image", json!({"src": "img.png"}))
}

pub fn br() -> Item {
    leaf("hard_break", Value::Null)
}

fn mark(name: &'static str, attrs_value: Value, children: Vec<Item>) -> Item {
    Item::Mark {
        name,
        attrs: attrs(attrs_value),
        children,
    }
}

pub fn em(children: Vec<Item>) -> Item {
    mark("em", Value::Null, children)
}

pub fn strong(children: Vec<Item>) -> Item {
    mark("strong", Value::Null, children)
}

pub fn code(children: Vec<Item>) -> Item {
    mark("code", Value::Null, children)
}

pub fn a(children: Vec<Item>) -> Item {
    mark("link", json!({"href": "foo"}), children)
}

/// Builds a node of any type in any schema, e.g. for custom test schemas.
pub fn node_in(schema: &Schema, name: &str, attrs_value: Value, children: Vec<Item>) -> TaggedNode {
    build_in(schema, name, attrs(attrs_value), children)
}

/// A child node item with attributes (built in the surrounding schema).
pub fn n(name: &'static str, attrs_value: Value, children: Vec<Item>) -> Item {
    Item::Node {
        name,
        attrs: attrs(attrs_value),
        children,
    }
}

/// A mark item with attributes wrapping `children`.
pub fn m(name: &'static str, attrs_value: Value, children: Vec<Item>) -> Item {
    mark(name, attrs_value, children)
}
