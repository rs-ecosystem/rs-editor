use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::content::{self, ContentMatch, MatchState};
use crate::error::{ModelError, Result};
use crate::fragment::Fragment;
use crate::mark::{Mark, MarkSet};
use crate::node::Node;
use crate::text::utf16_len;

/// Attribute values of a node or mark, shared between copies.
pub type Attrs = Arc<Map<String, Value>>;

/// Declares one attribute of a node or mark type.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AttrSpec {
    /// Value used when the attribute is not given. Without a default the
    /// attribute is required, which also makes the type non-generatable.
    pub default: Option<Value>,
    /// Allowed JSON types separated by `|`: `string`, `number`, `boolean`,
    /// `null`, `array`, `object`. `None` accepts anything.
    pub validate: Option<String>,
}

impl AttrSpec {
    pub fn required() -> Self {
        Self::default()
    }

    pub fn with_default(default: impl Into<Value>) -> Self {
        Self {
            default: Some(default.into()),
            validate: None,
        }
    }

    pub fn validate(mut self, types: &str) -> Self {
        self.validate = Some(types.to_owned());
        self
    }
}

/// Describes a node type. Mirrors ProseMirror's `NodeSpec`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeSpec {
    /// Content expression such as `"block+"` or `"paragraph block*"`.
    /// `None` makes the node a leaf.
    pub content: Option<String>,
    /// Allowed marks: `"_"` for all, `""` for none, or names and groups.
    /// Defaults to all marks for inline content and none otherwise.
    pub marks: Option<String>,
    /// Space separated groups usable in content expressions.
    pub group: Option<String>,
    pub inline: bool,
    pub atom: bool,
    pub attrs: Vec<(String, AttrSpec)>,
    pub selectable: Option<bool>,
    pub draggable: bool,
    /// Content is code: whitespace is preserved and marks are usually off.
    pub code: bool,
    /// Replacing the full content keeps this node (e.g. a list item).
    pub defining: bool,
    /// Editing operations do not cross this node's boundary (e.g. table cells).
    pub isolating: bool,
    /// Plain text a leaf contributes to `text_between`, e.g. `"\n"` for a hard break.
    pub leaf_text: Option<String>,
}

impl NodeSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn content(mut self, expr: &str) -> Self {
        self.content = Some(expr.to_owned());
        self
    }

    pub fn marks(mut self, marks: &str) -> Self {
        self.marks = Some(marks.to_owned());
        self
    }

    pub fn group(mut self, group: &str) -> Self {
        self.group = Some(group.to_owned());
        self
    }

    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }

    pub fn atom(mut self) -> Self {
        self.atom = true;
        self
    }

    pub fn attr(mut self, name: &str, spec: AttrSpec) -> Self {
        self.attrs.push((name.to_owned(), spec));
        self
    }

    pub fn code(mut self) -> Self {
        self.code = true;
        self
    }

    pub fn defining(mut self) -> Self {
        self.defining = true;
        self
    }

    pub fn isolating(mut self) -> Self {
        self.isolating = true;
        self
    }

    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = Some(selectable);
        self
    }

    pub fn draggable(mut self) -> Self {
        self.draggable = true;
        self
    }

    pub fn leaf_text(mut self, text: &str) -> Self {
        self.leaf_text = Some(text.to_owned());
        self
    }
}

/// Describes a mark type. Mirrors ProseMirror's `MarkSpec`.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkSpec {
    pub attrs: Vec<(String, AttrSpec)>,
    /// Typing at the mark's end extends it. Links set this to `false`.
    pub inclusive: bool,
    /// Marks this one cannot coexist with: `None` excludes only itself,
    /// `""` nothing, `"_"` everything, otherwise names and groups.
    pub excludes: Option<String>,
    pub group: Option<String>,
    /// The mark spans adjacent nodes when serialized (e.g. links).
    pub spanning: bool,
    /// Typing at a code-like mark keeps marks out of other formatting.
    pub code: bool,
}

impl Default for MarkSpec {
    fn default() -> Self {
        Self {
            attrs: Vec::new(),
            inclusive: true,
            excludes: None,
            group: None,
            spanning: true,
            code: false,
        }
    }
}

impl MarkSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attr(mut self, name: &str, spec: AttrSpec) -> Self {
        self.attrs.push((name.to_owned(), spec));
        self
    }

    pub fn inclusive(mut self, inclusive: bool) -> Self {
        self.inclusive = inclusive;
        self
    }

    pub fn excludes(mut self, excludes: &str) -> Self {
        self.excludes = Some(excludes.to_owned());
        self
    }

    pub fn group(mut self, group: &str) -> Self {
        self.group = Some(group.to_owned());
        self
    }

    pub fn code(mut self) -> Self {
        self.code = true;
        self
    }
}

/// Ordered node and mark declarations. Order matters: the first block type
/// is the default for generated content and marks are ranked by position.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SchemaSpec {
    pub nodes: Vec<(String, NodeSpec)>,
    pub marks: Vec<(String, MarkSpec)>,
    /// Defaults to `"doc"`.
    pub top_node: Option<String>,
}

impl SchemaSpec {
    pub fn node(mut self, name: &str, spec: NodeSpec) -> Self {
        self.nodes.push((name.to_owned(), spec));
        self
    }

    pub fn mark(mut self, name: &str, spec: MarkSpec) -> Self {
        self.marks.push((name.to_owned(), spec));
        self
    }
}

#[derive(Debug)]
pub(crate) struct NodeTypeData {
    pub name: String,
    pub spec: NodeSpec,
    pub groups: Vec<String>,
    pub is_block: bool,
    pub is_text: bool,
    pub content_match: u32,
    pub inline_content: bool,
    /// `None` allows every mark.
    pub mark_set: Option<Vec<u16>>,
    pub default_attrs: Option<Attrs>,
}

#[derive(Debug)]
pub(crate) struct MarkTypeData {
    pub name: String,
    pub spec: MarkSpec,
    pub groups: Vec<String>,
    pub excluded: Vec<u16>,
    pub default_attrs: Option<Attrs>,
}

#[derive(Debug)]
pub(crate) struct SchemaInner {
    pub nodes: Vec<NodeTypeData>,
    pub marks: Vec<MarkTypeData>,
    pub node_names: HashMap<String, u16>,
    pub mark_names: HashMap<String, u16>,
    pub matches: Vec<MatchState>,
    pub top: u16,
    pub text: u16,
}

/// A compiled schema. Cheap to clone; all node and mark types refer back to it.
#[derive(Clone)]
pub struct Schema(pub(crate) Arc<SchemaInner>);

impl fmt::Debug for Schema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Schema")
            .field(
                "nodes",
                &self.0.nodes.iter().map(|n| &n.name).collect::<Vec<_>>(),
            )
            .field(
                "marks",
                &self.0.marks.iter().map(|m| &m.name).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl PartialEq for Schema {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Schema {}

fn split_words(value: &Option<String>) -> Vec<String> {
    value
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

fn default_attrs(owner: &str, attrs: &[(String, AttrSpec)]) -> Result<Option<Attrs>> {
    let mut seen = std::collections::HashSet::new();
    let mut map = Map::new();
    let mut complete = true;
    for (name, spec) in attrs {
        if !seen.insert(name) {
            return Err(ModelError::Schema(format!(
                "duplicate attribute `{name}` on `{owner}`"
            )));
        }
        match &spec.default {
            Some(value) => {
                check_attr_type(owner, name, spec, value)?;
                map.insert(name.clone(), value.clone());
            }
            None => complete = false,
        }
    }
    Ok(complete.then(|| Arc::new(map)))
}

fn check_attr_type(owner: &str, name: &str, spec: &AttrSpec, value: &Value) -> Result<()> {
    let Some(types) = &spec.validate else {
        return Ok(());
    };
    let actual = match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    if types.split('|').any(|ty| ty.trim() == actual) {
        Ok(())
    } else {
        Err(ModelError::InvalidAttr {
            owner: owner.to_owned(),
            attr: name.to_owned(),
            reason: format!("expected {types}, got {actual}"),
        })
    }
}

/// Builds the attribute map for `specs` from `given`, filling defaults.
/// Attributes the type does not declare are dropped, as in ProseMirror.
fn compute_attrs(
    owner: &str,
    specs: &[(String, AttrSpec)],
    defaults: &Option<Attrs>,
    given: Option<&Map<String, Value>>,
) -> Result<Attrs> {
    if given.is_none_or(Map::is_empty)
        && let Some(defaults) = defaults
    {
        return Ok(defaults.clone());
    }
    let mut built = Map::new();
    for (name, spec) in specs {
        let value = match given.and_then(|given| given.get(name)) {
            Some(value) => value.clone(),
            None => spec
                .default
                .clone()
                .ok_or_else(|| ModelError::MissingAttr {
                    owner: owner.to_owned(),
                    attr: name.clone(),
                })?,
        };
        check_attr_type(owner, name, spec, &value)?;
        built.insert(name.clone(), value);
    }
    Ok(Arc::new(built))
}

impl Schema {
    pub fn new(spec: SchemaSpec) -> Result<Self> {
        let top_name = spec.top_node.clone().unwrap_or_else(|| "doc".to_owned());
        let mut node_names = HashMap::new();
        let mut nodes = Vec::with_capacity(spec.nodes.len());
        for (index, (name, node)) in spec.nodes.iter().enumerate() {
            let id = u16::try_from(index)
                .map_err(|_| ModelError::Schema("too many node types".into()))?;
            if node_names.insert(name.clone(), id).is_some() {
                return Err(ModelError::Schema(format!("duplicate node type `{name}`")));
            }
            let is_text = name == "text";
            if is_text && !node.attrs.is_empty() {
                return Err(ModelError::Schema(
                    "the text node type should not have attributes".into(),
                ));
            }
            nodes.push(NodeTypeData {
                name: name.clone(),
                groups: split_words(&node.group),
                is_block: !(node.inline || is_text),
                is_text,
                content_match: 0,
                inline_content: false,
                mark_set: None,
                default_attrs: default_attrs(name, &node.attrs)?,
                spec: node.clone(),
            });
        }
        let top = *node_names
            .get(&top_name)
            .ok_or_else(|| ModelError::Schema(format!("missing top node type `{top_name}`")))?;
        let text = *node_names
            .get("text")
            .ok_or_else(|| ModelError::Schema("every schema needs a `text` type".into()))?;

        let mut marks = Vec::with_capacity(spec.marks.len());
        let mut mark_names = HashMap::new();
        for (index, (name, mark)) in spec.marks.iter().enumerate() {
            let id = u16::try_from(index)
                .map_err(|_| ModelError::Schema("too many mark types".into()))?;
            if mark_names.insert(name.clone(), id).is_some() {
                return Err(ModelError::Schema(format!("duplicate mark type `{name}`")));
            }
            marks.push(MarkTypeData {
                name: name.clone(),
                groups: split_words(&mark.group),
                excluded: Vec::new(),
                default_attrs: default_attrs(name, &mark.attrs)?,
                spec: mark.clone(),
            });
        }
        for index in 0..marks.len() {
            let excluded = match marks[index].spec.excludes.as_deref() {
                None => vec![index as u16],
                Some(expr) => gather_marks(&marks, &mark_names, expr)?,
            };
            marks[index].excluded = excluded;
        }

        let mut matches = vec![MatchState::empty()];
        let mut compiled: HashMap<String, u32> = HashMap::new();
        for index in 0..nodes.len() {
            let expr = nodes[index].spec.content.clone().unwrap_or_default();
            let start = match compiled.get(&expr) {
                Some(start) => *start,
                None => {
                    let start = content::compile(&expr, &nodes, &node_names, &mut matches)
                        .map_err(|message| {
                            ModelError::Schema(format!(
                                "content of `{}`: {message}",
                                nodes[index].name
                            ))
                        })?;
                    compiled.insert(expr, start);
                    start
                }
            };
            nodes[index].content_match = start;
            nodes[index].inline_content = matches[start as usize]
                .next
                .first()
                .is_some_and(|(ty, _)| !nodes[*ty as usize].is_block);
        }
        for node in &mut nodes {
            node.mark_set = match node.spec.marks.as_deref() {
                Some("_") => None,
                Some(expr) => Some(gather_marks(&marks, &mark_names, expr)?),
                None if node.inline_content => None,
                None => Some(Vec::new()),
            };
        }
        if !nodes[top as usize].is_block {
            return Err(ModelError::Schema("the top node must be a block".into()));
        }
        Ok(Self(Arc::new(SchemaInner {
            nodes,
            marks,
            node_names,
            mark_names,
            matches,
            top,
            text,
        })))
    }

    pub fn node_type(&self, name: &str) -> Result<NodeType> {
        self.0
            .node_names
            .get(name)
            .map(|id| NodeType {
                schema: self.clone(),
                id: *id,
            })
            .ok_or_else(|| ModelError::UnknownNodeType(name.to_owned()))
    }

    pub fn mark_type(&self, name: &str) -> Result<MarkType> {
        self.0
            .mark_names
            .get(name)
            .map(|id| MarkType {
                schema: self.clone(),
                id: *id,
            })
            .ok_or_else(|| ModelError::UnknownMarkType(name.to_owned()))
    }

    pub fn node_types(&self) -> impl Iterator<Item = NodeType> + '_ {
        (0..self.0.nodes.len()).map(|id| NodeType {
            schema: self.clone(),
            id: id as u16,
        })
    }

    pub fn mark_types(&self) -> impl Iterator<Item = MarkType> + '_ {
        (0..self.0.marks.len()).map(|id| MarkType {
            schema: self.clone(),
            id: id as u16,
        })
    }

    pub fn top_node_type(&self) -> NodeType {
        NodeType {
            schema: self.clone(),
            id: self.0.top,
        }
    }

    pub fn text_type(&self) -> NodeType {
        NodeType {
            schema: self.clone(),
            id: self.0.text,
        }
    }

    /// Creates a text node. Empty text is rejected.
    pub fn text(&self, text: impl Into<Arc<str>>, marks: MarkSet) -> Result<Node> {
        let text: Arc<str> = text.into();
        if text.is_empty() {
            return Err(ModelError::EmptyText);
        }
        let len = utf16_len(&text);
        Ok(Node::new_text(self.text_type(), text, len, marks))
    }

    pub(crate) fn content_match(&self, state: u32) -> ContentMatch {
        ContentMatch {
            schema: self.clone(),
            state,
        }
    }
}

fn gather_marks(
    marks: &[MarkTypeData],
    names: &HashMap<String, u16>,
    expr: &str,
) -> Result<Vec<u16>> {
    let mut found = Vec::new();
    for word in expr.split_whitespace() {
        let mut ok = false;
        if let Some(id) = names.get(word) {
            found.push(*id);
            ok = true;
        } else {
            for (id, mark) in marks.iter().enumerate() {
                if word == "_" || mark.groups.iter().any(|group| group == word) {
                    found.push(id as u16);
                    ok = true;
                }
            }
        }
        if !ok {
            return Err(ModelError::Schema(format!(
                "unknown mark type or group `{word}`"
            )));
        }
    }
    found.sort_unstable();
    found.dedup();
    Ok(found)
}

/// A node type of a [`Schema`].
#[derive(Clone)]
pub struct NodeType {
    pub(crate) schema: Schema,
    pub(crate) id: u16,
}

impl PartialEq for NodeType {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.schema == other.schema
    }
}

impl Eq for NodeType {}

impl std::hash::Hash for NodeType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl fmt::Debug for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeType({})", self.name())
    }
}

impl NodeType {
    pub(crate) fn data(&self) -> &NodeTypeData {
        &self.schema.0.nodes[self.id as usize]
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn name(&self) -> &str {
        &self.data().name
    }

    pub fn spec(&self) -> &NodeSpec {
        &self.data().spec
    }

    pub fn is_in_group(&self, group: &str) -> bool {
        self.data().groups.iter().any(|g| g == group)
    }

    pub fn is_text(&self) -> bool {
        self.data().is_text
    }

    pub fn is_block(&self) -> bool {
        self.data().is_block
    }

    pub fn is_inline(&self) -> bool {
        !self.is_block()
    }

    pub fn is_textblock(&self) -> bool {
        self.is_block() && self.inline_content()
    }

    pub fn inline_content(&self) -> bool {
        self.data().inline_content
    }

    pub fn is_leaf(&self) -> bool {
        self.data().content_match == 0
    }

    pub fn is_atom(&self) -> bool {
        self.is_leaf() || self.spec().atom
    }

    pub fn has_required_attrs(&self) -> bool {
        self.data().default_attrs.is_none()
    }

    pub fn content_match(&self) -> ContentMatch {
        self.schema.content_match(self.data().content_match)
    }

    pub fn compatible_content(&self, other: &NodeType) -> bool {
        self == other || self.content_match().compatible(&other.content_match())
    }

    pub fn compute_attrs(&self, given: Option<&Map<String, Value>>) -> Result<Attrs> {
        let data = self.data();
        compute_attrs(&data.name, &data.spec.attrs, &data.default_attrs, given)
    }

    pub fn check_attrs(&self, attrs: &Map<String, Value>) -> Result<()> {
        let data = self.data();
        for (name, value) in attrs {
            let Some((_, spec)) = data.spec.attrs.iter().find(|(attr, _)| attr == name) else {
                return Err(ModelError::InvalidAttr {
                    owner: data.name.clone(),
                    attr: name.clone(),
                    reason: "unsupported attribute".into(),
                });
            };
            check_attr_type(&data.name, name, spec, value)?;
        }
        Ok(())
    }

    pub fn allows_mark_type(&self, mark: &MarkType) -> bool {
        self.data()
            .mark_set
            .as_ref()
            .is_none_or(|set| set.binary_search(&mark.id).is_ok())
    }

    pub fn allows_marks(&self, marks: &[Mark]) -> bool {
        marks
            .iter()
            .all(|mark| self.allows_mark_type(mark.mark_type()))
    }

    /// Removes the marks this type does not allow.
    pub fn allowed_marks(&self, marks: &MarkSet) -> MarkSet {
        if self.data().mark_set.is_none() || self.allows_marks(marks) {
            return marks.clone();
        }
        MarkSet::from_sorted(
            marks
                .iter()
                .filter(|mark| self.allows_mark_type(mark.mark_type()))
                .cloned()
                .collect(),
        )
    }

    /// Creates a node without checking its content.
    pub fn create(
        &self,
        attrs: Option<&Map<String, Value>>,
        content: Fragment,
        marks: MarkSet,
    ) -> Result<Node> {
        if self.is_text() {
            return Err(ModelError::Schema(
                "use Schema::text to create text nodes".into(),
            ));
        }
        Ok(Node::new(
            self.clone(),
            self.compute_attrs(attrs)?,
            content,
            marks,
        ))
    }

    /// Creates a node and checks that its content and marks are valid.
    pub fn create_checked(
        &self,
        attrs: Option<&Map<String, Value>>,
        content: Fragment,
        marks: MarkSet,
    ) -> Result<Node> {
        self.check_content(&content)?;
        self.create(attrs, content, marks)
    }

    /// Creates a node, adding nodes before and after `content` as needed to
    /// make it valid. Returns `None` when that is impossible.
    pub fn create_and_fill(
        &self,
        attrs: Option<&Map<String, Value>>,
        content: Fragment,
        marks: MarkSet,
    ) -> Option<Node> {
        let attrs = self.compute_attrs(attrs).ok()?;
        let mut content = content;
        if content.size() > 0 {
            let before = self.content_match().fill_before(&content, false, 0)?;
            content = before.append(&content);
        }
        let matched = self
            .content_match()
            .match_fragment(&content, 0, content.child_count())?;
        let after = matched.fill_before(&Fragment::empty(), true, 0)?;
        Some(Node::new(
            self.clone(),
            attrs,
            content.append(&after),
            marks,
        ))
    }

    pub fn valid_content(&self, content: &Fragment) -> bool {
        let Some(result) = self
            .content_match()
            .match_fragment(content, 0, content.child_count())
        else {
            return false;
        };
        result.valid_end() && content.iter().all(|child| self.allows_marks(child.marks()))
    }

    pub fn check_content(&self, content: &Fragment) -> Result<()> {
        if self.valid_content(content) {
            Ok(())
        } else {
            Err(ModelError::InvalidContent(self.name().to_owned()))
        }
    }
}

/// A mark type of a [`Schema`].
#[derive(Clone)]
pub struct MarkType {
    pub(crate) schema: Schema,
    pub(crate) id: u16,
}

impl PartialEq for MarkType {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.schema == other.schema
    }
}

impl Eq for MarkType {}

impl std::hash::Hash for MarkType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl fmt::Debug for MarkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MarkType({})", self.name())
    }
}

impl MarkType {
    pub(crate) fn data(&self) -> &MarkTypeData {
        &self.schema.0.marks[self.id as usize]
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn name(&self) -> &str {
        &self.data().name
    }

    pub fn spec(&self) -> &MarkSpec {
        &self.data().spec
    }

    /// Position in the schema; mark sets are ordered by rank.
    pub fn rank(&self) -> u16 {
        self.id
    }

    pub fn is_in_group(&self, group: &str) -> bool {
        self.data().groups.iter().any(|g| g == group)
    }

    pub fn excludes(&self, other: &MarkType) -> bool {
        self.data().excluded.contains(&other.id)
    }

    pub fn compute_attrs(&self, given: Option<&Map<String, Value>>) -> Result<Attrs> {
        let data = self.data();
        compute_attrs(&data.name, &data.spec.attrs, &data.default_attrs, given)
    }

    pub fn create(&self, attrs: Option<&Map<String, Value>>) -> Result<Mark> {
        Ok(Mark::new(self.clone(), self.compute_attrs(attrs)?))
    }

    /// The first mark of this type in `set`.
    pub fn is_in_set<'a>(&self, set: &'a [Mark]) -> Option<&'a Mark> {
        set.iter().find(|mark| mark.mark_type() == self)
    }

    pub fn remove_from_set(&self, set: &MarkSet) -> MarkSet {
        if self.is_in_set(set).is_none() {
            return set.clone();
        }
        MarkSet::from_sorted(
            set.iter()
                .filter(|m| m.mark_type() != self)
                .cloned()
                .collect(),
        )
    }
}
