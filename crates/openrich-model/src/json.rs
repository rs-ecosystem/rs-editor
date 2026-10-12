//! Tiptap/ProseMirror JSON: `{"type", "attrs"?, "content"?, "marks"?, "text"?}`.

use serde_json::{Map, Value, json};

use crate::error::{ModelError, Result};
use crate::fragment::Fragment;
use crate::mark::{Mark, MarkSet};
use crate::node::Node;
use crate::schema::Schema;

fn json_error(message: impl Into<String>) -> ModelError {
    ModelError::Json(message.into())
}

impl Mark {
    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("type".into(), json!(self.mark_type().name()));
        if !self.attrs().is_empty() {
            object.insert("attrs".into(), Value::Object((**self.attrs()).clone()));
        }
        Value::Object(object)
    }
}

impl Node {
    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("type".into(), json!(self.type_name()));
        if !self.attrs().is_empty() {
            object.insert("attrs".into(), Value::Object((**self.attrs()).clone()));
        }
        if let Some(text) = self.text() {
            object.insert("text".into(), json!(text));
        } else if self.content().size() > 0 {
            object.insert("content".into(), fragment_to_json(self.content()));
        }
        if !self.marks().is_empty() {
            object.insert(
                "marks".into(),
                Value::Array(self.marks().iter().map(Mark::to_json).collect()),
            );
        }
        Value::Object(object)
    }
}

pub fn fragment_to_json(fragment: &Fragment) -> Value {
    Value::Array(fragment.iter().map(Node::to_json).collect())
}

fn attrs_of(object: &Map<String, Value>) -> Result<Option<&Map<String, Value>>> {
    match object.get("attrs") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(attrs)) => Ok(Some(attrs)),
        Some(_) => Err(json_error("`attrs` must be an object")),
    }
}

impl Schema {
    pub fn mark_from_json(&self, value: &Value) -> Result<Mark> {
        let object = value
            .as_object()
            .ok_or_else(|| json_error("mark must be an object"))?;
        let name = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| json_error("mark needs a string `type`"))?;
        self.mark_type(name)?.create(attrs_of(object)?)
    }

    fn marks_from_json(&self, object: &Map<String, Value>) -> Result<MarkSet> {
        match object.get("marks") {
            None | Some(Value::Null) => Ok(MarkSet::none()),
            Some(Value::Array(marks)) => Ok(MarkSet::from_marks(
                marks
                    .iter()
                    .map(|mark| self.mark_from_json(mark))
                    .collect::<Result<Vec<_>>>()?,
            )),
            Some(_) => Err(json_error("`marks` must be an array")),
        }
    }

    pub fn fragment_from_json(&self, value: &Value) -> Result<Fragment> {
        match value {
            Value::Null => Ok(Fragment::empty()),
            Value::Array(items) => Ok(Fragment::from_array(
                items
                    .iter()
                    .map(|item| self.node_from_json_unchecked(item))
                    .collect::<Result<Vec<_>>>()?,
            )),
            _ => Err(json_error("`content` must be an array")),
        }
    }

    /// Parses a node without validating its content against the schema.
    pub fn node_from_json_unchecked(&self, value: &Value) -> Result<Node> {
        let object = value
            .as_object()
            .ok_or_else(|| json_error("node must be an object"))?;
        let name = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| json_error("node needs a string `type`"))?;
        let marks = self.marks_from_json(object)?;
        if name == self.text_type().name() {
            let text = object
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| json_error("text node needs a string `text`"))?;
            return self.text(text, marks);
        }
        let content = self.fragment_from_json(object.get("content").unwrap_or(&Value::Null))?;
        self.node_type(name)?
            .create(attrs_of(object)?, content, marks)
    }

    /// Parses and fully validates a node, typically a whole document.
    pub fn node_from_json(&self, value: &Value) -> Result<Node> {
        let node = self.node_from_json_unchecked(value)?;
        node.check()?;
        Ok(node)
    }
}
