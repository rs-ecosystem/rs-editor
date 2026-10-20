//! The headless editor: state plus extensions behind a small, FFI-friendly
//! API (named commands with JSON arguments, JSON content, events).

use std::fmt;
use std::sync::Arc;

use rs_editor_commands::{KeyEvent, KeyPlatform};
use rs_editor_model::Node;
use rs_editor_state::{
    Clock, Command, EditorState, EditorStateConfig, Selection, Transaction, ViewContext, reborrow,
};
use serde_json::{Map, Value};

use crate::extension::{Extension, ExtensionError, ExtensionManager};
use crate::helpers::{
    chain, get_mark_attributes, get_node_attributes, is_mark_active, is_node_active,
};
use crate::inputrules::run_input_rules;

/// What happened when transactions were applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorEvent {
    /// Any transaction was applied.
    Transaction,
    /// The document changed.
    Update,
    /// The selection changed without a document change.
    SelectionUpdate,
}

/// Called after every applied transaction batch.
pub type Listener = Box<dyn FnMut(EditorEvent, &EditorState) + Send>;

/// Options for [`Editor::new`].
pub struct EditorOptions {
    pub extensions: Vec<Arc<dyn Extension>>,
    /// Initial content as Tiptap JSON (`{"type": "doc", ...}`).
    pub content: Option<Value>,
    pub platform: KeyPlatform,
    pub clock: Option<Arc<dyn Clock>>,
    /// When false, only commands run by the host change the document;
    /// keys and typing are ignored.
    pub editable: bool,
}

impl fmt::Debug for EditorOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditorOptions")
            .field(
                "extensions",
                &self
                    .extensions
                    .iter()
                    .map(|e| e.name().to_owned())
                    .collect::<Vec<_>>(),
            )
            .field("platform", &self.platform)
            .field("editable", &self.editable)
            .finish()
    }
}

impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            extensions: crate::starter_kit::starter_kit(),
            content: None,
            platform: KeyPlatform::Other,
            clock: None,
            editable: true,
        }
    }
}

/// A rich text editor without a view.
pub struct Editor {
    manager: Arc<ExtensionManager>,
    state: EditorState,
    editable: bool,
    listeners: Vec<Listener>,
}

impl fmt::Debug for Editor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Editor")
            .field("state", &self.state)
            .field("editable", &self.editable)
            .finish()
    }
}

/// Builds a [`chain`] of named commands: `editor.chain().cmd("toggleBold", Value::Null).run()`.
pub struct ChainBuilder<'a> {
    editor: &'a mut Editor,
    commands: Vec<Result<Command, ExtensionError>>,
}

impl ChainBuilder<'_> {
    pub fn cmd(mut self, name: &str, args: Value) -> Self {
        self.commands.push(self.editor.manager.command(name, &args));
        self
    }

    /// Adds an already built command.
    pub fn command(mut self, command: Command) -> Self {
        self.commands.push(Ok(command));
        self
    }

    /// Runs the chain as one transaction. `Ok(false)` if any step did not apply.
    pub fn run(self) -> Result<bool, ExtensionError> {
        let commands = self.commands.into_iter().collect::<Result<Vec<_>, _>>()?;
        Ok(self.editor.exec(&chain(commands), None))
    }

    /// Whether the whole chain would apply.
    pub fn can(self) -> Result<bool, ExtensionError> {
        let commands = self.commands.into_iter().collect::<Result<Vec<_>, _>>()?;
        Ok(chain(commands)(&self.editor.state, None, None))
    }
}

impl Editor {
    pub fn new(options: EditorOptions) -> Result<Self, ExtensionError> {
        let manager = Arc::new(ExtensionManager::new(options.extensions, options.platform)?);
        let doc = match &options.content {
            Some(content) => Some(manager.schema().node_from_json(content)?),
            None => None,
        };
        let state = EditorState::create(EditorStateConfig {
            schema: Some(manager.schema().clone()),
            doc,
            plugins: manager.plugins().to_vec(),
            clock: options.clock,
            ..Default::default()
        })
        .map_err(|error| ExtensionError::State(error.to_string()))?;
        Ok(Self {
            manager,
            state,
            editable: options.editable,
            listeners: Vec::new(),
        })
    }

    pub fn state(&self) -> &EditorState {
        &self.state
    }

    pub fn manager(&self) -> &ExtensionManager {
        &self.manager
    }

    pub fn schema(&self) -> &rs_editor_model::Schema {
        self.manager.schema()
    }

    pub fn is_editable(&self) -> bool {
        self.editable
    }

    pub fn set_editable(&mut self, editable: bool) {
        self.editable = editable;
    }

    pub fn on(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    /// Applies a transaction (and any transactions plugins append),
    /// notifying listeners. Returns the applied transactions.
    pub fn dispatch(&mut self, tr: Transaction) -> Result<Vec<Transaction>, ExtensionError> {
        let selection_before = self.state.selection().clone();
        let applied = self
            .state
            .apply_transaction(tr)
            .map_err(|error| ExtensionError::State(error.to_string()))?;
        if applied.transactions.is_empty() {
            return Ok(applied.transactions);
        }
        self.state = applied.state;
        let doc_changed = applied.transactions.iter().any(|tr| tr.doc_changed());
        let mut events = vec![EditorEvent::Transaction];
        if doc_changed {
            events.push(EditorEvent::Update);
        } else if self.state.selection() != &selection_before {
            events.push(EditorEvent::SelectionUpdate);
        }
        for event in events {
            for listener in &mut self.listeners {
                listener(event, &self.state);
            }
        }
        Ok(applied.transactions)
    }

    /// Runs `command` against the current state, dispatching what it produces.
    pub fn exec(&mut self, command: &Command, view: Option<&dyn ViewContext>) -> bool {
        let mut produced = Vec::new();
        let applied = command(&self.state, Some(&mut |tr| produced.push(tr)), view);
        for tr in produced {
            if self.dispatch(tr).is_err() {
                return false;
            }
        }
        applied
    }

    /// Runs a named command, e.g. `run("toggleHeading", json!({"level": 2}))`.
    pub fn run(&mut self, name: &str, args: &Value) -> Result<bool, ExtensionError> {
        let command = self.manager.command(name, args)?;
        Ok(self.exec(&command, None))
    }

    /// Whether a named command would apply now (for toolbar enablement).
    pub fn can(&self, name: &str, args: &Value) -> bool {
        self.manager
            .command(name, args)
            .is_ok_and(|command| command(&self.state, None, None))
    }

    pub fn chain(&mut self) -> ChainBuilder<'_> {
        ChainBuilder {
            editor: self,
            commands: Vec::new(),
        }
    }

    /// Whether the node or mark `name` (with matching `attrs`) is active
    /// at the selection, for toolbar state.
    pub fn is_active(&self, name: &str, attrs: &Map<String, Value>) -> bool {
        let schema = self.schema();
        if let Ok(mark) = schema.mark_type(name) {
            return is_mark_active(&self.state, &mark, attrs);
        }
        if let Ok(node) = schema.node_type(name) {
            return is_node_active(&self.state, &node, attrs);
        }
        false
    }

    /// Attributes of the node or mark `name` at the selection.
    pub fn get_attributes(&self, name: &str) -> Map<String, Value> {
        let schema = self.schema();
        if let Ok(mark) = schema.mark_type(name) {
            return get_mark_attributes(&self.state, &mark);
        }
        if let Ok(node) = schema.node_type(name) {
            return get_node_attributes(&self.state, &node);
        }
        Map::new()
    }

    /// The document as Tiptap JSON.
    pub fn get_json(&self) -> Value {
        self.state.doc().to_json()
    }

    /// Plain text, with `block_separator` between blocks.
    pub fn get_text(&self, block_separator: &str) -> String {
        let doc = self.state.doc();
        doc.text_between(0, doc.content().size(), Some(block_separator), None)
    }

    /// True for a document with a single empty paragraph.
    pub fn is_empty(&self) -> bool {
        let doc = self.state.doc();
        doc.child_count() == 1 && doc.child(0).is_textblock() && doc.child(0).content().size() == 0
    }

    /// Replaces the document with Tiptap JSON.
    pub fn set_content(&mut self, content: &Value) -> Result<bool, ExtensionError> {
        self.run("setContent", content)
    }

    /// Runs the key bindings for `event`. Returns whether one handled it.
    pub fn handle_key(&mut self, event: &KeyEvent, view: Option<&dyn ViewContext>) -> bool {
        if !self.editable {
            return false;
        }
        let manager = self.manager.clone();
        let mut produced = Vec::new();
        let handled = {
            let mut collect = |tr: Transaction| produced.push(tr);
            let mut dispatch: rs_editor_state::Dispatch = Some(&mut collect);
            manager
                .keymaps()
                .iter()
                .any(|keymap| keymap.handle(event, &self.state, reborrow(&mut dispatch), view))
        };
        for tr in produced {
            if self.dispatch(tr).is_err() {
                return false;
            }
        }
        handled
    }

    /// Text typed (or committed by an IME) over `from..to`: runs input
    /// rules first, then inserts the text with the active marks.
    pub fn handle_text_input(&mut self, from: usize, to: usize, text: &str) -> bool {
        if !self.editable || text.is_empty() {
            return false;
        }
        if let Some(tr) = run_input_rules(self.manager.input_rules(), &self.state, from, to, text) {
            return self.dispatch(tr).is_ok();
        }
        let mut tr = self.state.tr();
        if tr.insert_text_at(text, from, to).is_err() {
            return false;
        }
        tr.scroll_into_view();
        self.dispatch(tr).is_ok()
    }

    /// Typed text over the current selection.
    pub fn type_text(&mut self, text: &str) -> bool {
        let selection = self.state.selection();
        let (from, to) = (selection.from(), selection.to());
        self.handle_text_input(from, to, text)
    }

    /// Sets the selection to text `anchor..head`.
    pub fn select(&mut self, anchor: usize, head: usize) -> bool {
        let doc = self.state.doc();
        let size = doc.content().size();
        let (Ok(a), Ok(h)) = (doc.resolve(anchor.min(size)), doc.resolve(head.min(size))) else {
            return false;
        };
        let selection = rs_editor_state::TextSelection::between(&a, &h, None);
        let mut tr = self.state.tr();
        tr.set_selection(selection);
        self.dispatch(tr).is_ok()
    }

    pub fn selection(&self) -> &Selection {
        self.state.selection()
    }

    pub fn doc(&self) -> &Node {
        self.state.doc()
    }
}

impl fmt::Debug for ChainBuilder<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChainBuilder")
            .field("steps", &self.commands.len())
            .finish()
    }
}
