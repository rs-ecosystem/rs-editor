//! Small helpers shared by the bundled extensions.

use std::sync::Arc;

use rs_editor_model::{MarkType, NodeType, Schema};
use rs_editor_state::Command;
use serde_json::{Map, Value};

use crate::extension::{CommandFactory, ExtensionError};
use crate::inputrules::{RuleAttrs, RuleMatch};

/// A factory for a command that takes no arguments.
pub fn no_args(cmd: Command) -> CommandFactory {
    Arc::new(move |_: &Value| Ok(cmd.clone()))
}

/// A factory whose command is built from the JSON arguments.
pub fn with_args(
    f: impl Fn(&Value) -> Result<Command, ExtensionError> + Send + Sync + 'static,
) -> CommandFactory {
    Arc::new(f)
}

/// Arguments as an attribute map: `{"level": 2}` or `{"attrs": {...}}`.
pub fn attrs_of(args: &Value) -> Map<String, Value> {
    match args {
        Value::Object(map) => match map.get("attrs") {
            Some(Value::Object(attrs)) => attrs.clone(),
            _ => map.clone(),
        },
        _ => Map::new(),
    }
}

pub fn node(schema: &Schema, name: &str) -> NodeType {
    schema
        .node_type(name)
        .unwrap_or_else(|_| panic!("schema built from this extension has `{name}`"))
}

pub fn mark(schema: &Schema, name: &str) -> MarkType {
    schema
        .mark_type(name)
        .unwrap_or_else(|_| panic!("schema built from this extension has `{name}`"))
}

pub fn invalid(command: &str, reason: impl Into<String>) -> ExtensionError {
    ExtensionError::InvalidArguments {
        command: command.to_owned(),
        reason: reason.into(),
    }
}

/// Rule attributes from a closure.
pub fn rule_attrs(
    f: impl Fn(&RuleMatch) -> Option<Map<String, Value>> + Send + Sync + 'static,
) -> RuleAttrs {
    Arc::new(f)
}

pub fn json_map(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
