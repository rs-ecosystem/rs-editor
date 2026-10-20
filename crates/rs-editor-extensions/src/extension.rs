//! The extension system: extensions contribute schema, commands, key
//! bindings, input rules and plugins, like Tiptap extensions.

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use rs_editor_commands::{KeyPlatform, Keymap};
use rs_editor_model::{AttrSpec, MarkSpec, ModelError, NodeSpec, Schema, SchemaSpec};
use rs_editor_state::{Command, Plugin};
use serde_json::Value;
use thiserror::Error;

use crate::inputrules::{InputRule, InputRulesPlugin};

/// Problems building or running extensions.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExtensionError {
    #[error(transparent)]
    Schema(#[from] ModelError),
    #[error("two extensions define `{0}`")]
    Duplicate(String),
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("invalid arguments for `{command}`: {reason}")]
    InvalidArguments { command: String, reason: String },
    #[error("invalid key binding: {0}")]
    KeyBinding(String),
    #[error("{0}")]
    State(String),
}

/// Attributes an extension adds to node types defined by others (e.g.
/// `textAlign` on paragraphs and headings).
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalAttribute {
    pub types: Vec<String>,
    pub attrs: Vec<(String, AttrSpec)>,
}

/// Builds a command from JSON arguments. Named commands are the stable
/// cross-language API: hosts call `run("toggleHeading", {"level": 2})`.
pub type CommandFactory = Arc<dyn Fn(&Value) -> Result<Command, ExtensionError> + Send + Sync>;

/// What extensions can see when they build their contributions.
#[derive(Debug, Clone)]
pub struct ExtensionContext {
    pub schema: Schema,
    pub platform: KeyPlatform,
}

/// A unit of editor functionality. Every method has a no-op default.
pub trait Extension: Send + Sync + 'static {
    /// Unique name, e.g. `"bold"`.
    fn name(&self) -> &str;

    /// Higher priorities come first: their node types are listed earlier in
    /// the schema (so `paragraph` is the default block) and their key
    /// bindings and input rules are tried first.
    fn priority(&self) -> i32 {
        100
    }

    fn nodes(&self) -> Vec<(String, NodeSpec)> {
        Vec::new()
    }

    fn marks(&self) -> Vec<(String, MarkSpec)> {
        Vec::new()
    }

    fn global_attributes(&self) -> Vec<GlobalAttribute> {
        Vec::new()
    }

    fn commands(&self, _ctx: &ExtensionContext) -> Vec<(String, CommandFactory)> {
        Vec::new()
    }

    /// Bindings in ProseMirror syntax (`"Mod-b"`).
    fn keyboard_shortcuts(&self, _ctx: &ExtensionContext) -> Vec<(String, Command)> {
        Vec::new()
    }

    fn input_rules(&self, _ctx: &ExtensionContext) -> Vec<InputRule> {
        Vec::new()
    }

    fn plugins(&self, _ctx: &ExtensionContext) -> Vec<Arc<dyn Plugin>> {
        Vec::new()
    }

    fn as_any(&self) -> &dyn Any;
}

/// Everything the extensions contribute, resolved against one schema.
pub struct ExtensionManager {
    extensions: Vec<Arc<dyn Extension>>,
    schema: Schema,
    platform: KeyPlatform,
    commands: HashMap<String, CommandFactory>,
    keymaps: Vec<Keymap>,
    input_rules: Vec<InputRule>,
    plugins: Vec<Arc<dyn Plugin>>,
}

impl fmt::Debug for ExtensionManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut commands: Vec<_> = self.commands.keys().collect();
        commands.sort();
        f.debug_struct("ExtensionManager")
            .field(
                "extensions",
                &self
                    .extensions
                    .iter()
                    .map(|e| e.name().to_owned())
                    .collect::<Vec<_>>(),
            )
            .field("schema", &self.schema)
            .field("commands", &commands)
            .finish()
    }
}

fn apply_global_attributes(spec: &mut SchemaSpec, globals: &[GlobalAttribute]) {
    for global in globals {
        for (name, node) in &mut spec.nodes {
            if global.types.iter().any(|ty| ty == name) {
                for (attr, attr_spec) in &global.attrs {
                    if !node.attrs.iter().any(|(existing, _)| existing == attr) {
                        node.attrs.push((attr.clone(), attr_spec.clone()));
                    }
                }
            }
        }
        for (name, mark) in &mut spec.marks {
            if global.types.iter().any(|ty| ty == name) {
                for (attr, attr_spec) in &global.attrs {
                    if !mark.attrs.iter().any(|(existing, _)| existing == attr) {
                        mark.attrs.push((attr.clone(), attr_spec.clone()));
                    }
                }
            }
        }
    }
}

impl ExtensionManager {
    pub fn new(
        extensions: Vec<Arc<dyn Extension>>,
        platform: KeyPlatform,
    ) -> Result<Self, ExtensionError> {
        let mut extensions = extensions;
        // Stable: equal priorities keep their configured order.
        extensions.sort_by_key(|extension| std::cmp::Reverse(extension.priority()));
        for (index, extension) in extensions.iter().enumerate() {
            if extensions[..index]
                .iter()
                .any(|other| other.name() == extension.name())
            {
                return Err(ExtensionError::Duplicate(extension.name().to_owned()));
            }
        }

        let mut spec = SchemaSpec::default();
        for extension in &extensions {
            for (name, node) in extension.nodes() {
                if spec.nodes.iter().any(|(existing, _)| *existing == name) {
                    return Err(ExtensionError::Duplicate(name));
                }
                spec.nodes.push((name, node));
            }
            for (name, mark) in extension.marks() {
                if spec.marks.iter().any(|(existing, _)| *existing == name) {
                    return Err(ExtensionError::Duplicate(name));
                }
                spec.marks.push((name, mark));
            }
        }
        let globals: Vec<GlobalAttribute> = extensions
            .iter()
            .flat_map(|e| e.global_attributes())
            .collect();
        apply_global_attributes(&mut spec, &globals);
        let schema = Schema::new(spec)?;
        let ctx = ExtensionContext {
            schema: schema.clone(),
            platform,
        };

        let mut commands = HashMap::new();
        let mut keymaps = Vec::new();
        let mut input_rules = Vec::new();
        let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();
        for extension in &extensions {
            for (name, factory) in extension.commands(&ctx) {
                if commands.insert(name.clone(), factory).is_some() {
                    return Err(ExtensionError::Duplicate(name));
                }
            }
            let shortcuts = extension.keyboard_shortcuts(&ctx);
            if !shortcuts.is_empty() {
                let bindings = shortcuts
                    .iter()
                    .map(|(key, cmd)| (key.as_str(), cmd.clone()))
                    .collect();
                keymaps.push(Keymap::new(bindings, platform).map_err(ExtensionError::KeyBinding)?);
            }
            input_rules.extend(extension.input_rules(&ctx));
            plugins.extend(extension.plugins(&ctx));
        }
        plugins.insert(
            0,
            Arc::new(InputRulesPlugin {
                rules: input_rules.clone(),
            }),
        );
        Ok(Self {
            extensions,
            schema,
            platform,
            commands,
            keymaps,
            input_rules,
            plugins,
        })
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn platform(&self) -> KeyPlatform {
        self.platform
    }

    pub fn extensions(&self) -> &[Arc<dyn Extension>] {
        &self.extensions
    }

    /// The extension named `name`, downcast to its type (to read options).
    pub fn extension<T: Extension>(&self, name: &str) -> Option<&T> {
        self.extensions
            .iter()
            .find(|e| e.name() == name)
            .and_then(|e| e.as_any().downcast_ref::<T>())
    }

    pub fn has_extension(&self, name: &str) -> bool {
        self.extensions.iter().any(|e| e.name() == name)
    }

    pub fn plugins(&self) -> &[Arc<dyn Plugin>] {
        &self.plugins
    }

    /// Key bindings, highest priority first.
    pub fn keymaps(&self) -> &[Keymap] {
        &self.keymaps
    }

    pub fn input_rules(&self) -> &[InputRule] {
        &self.input_rules
    }

    pub fn command_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.commands.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// Instantiates the named command with JSON arguments.
    pub fn command(&self, name: &str, args: &Value) -> Result<Command, ExtensionError> {
        let factory = self
            .commands
            .get(name)
            .ok_or_else(|| ExtensionError::UnknownCommand(name.to_owned()))?;
        factory(args)
    }
}
