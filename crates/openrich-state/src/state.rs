//! The editor state (`state.ts`).

use std::fmt;
use std::sync::Arc;

use openrich_model::{MarkSet, ModelError, Node, Schema};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::plugin::{Clock, Plugin, PluginValue, SystemClock};
use crate::selection::Selection;
use crate::transaction::{Transaction, meta};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StateError {
    #[error("a schema or a document is required")]
    MissingSchema,
    #[error("two plugins use the key `{0}`")]
    DuplicatePlugin(String),
    #[error("the transaction was made for a different document")]
    MismatchedTransaction,
    #[error("the JSON field `{0}` is reserved")]
    ReservedField(String),
    #[error(transparent)]
    Model(#[from] ModelError),
}

/// Inputs for [`EditorState::create`].
#[derive(Default)]
#[allow(missing_debug_implementations)]
pub struct EditorStateConfig {
    pub schema: Option<Schema>,
    pub doc: Option<Node>,
    pub selection: Option<Selection>,
    pub stored_marks: Option<MarkSet>,
    pub plugins: Vec<Arc<dyn Plugin>>,
    pub clock: Option<Arc<dyn Clock>>,
}

struct Configuration {
    schema: Schema,
    plugins: Vec<Arc<dyn Plugin>>,
    clock: Arc<dyn Clock>,
}

impl Configuration {
    fn new(
        schema: Schema,
        plugins: Vec<Arc<dyn Plugin>>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, StateError> {
        for (index, plugin) in plugins.iter().enumerate() {
            if plugins[..index]
                .iter()
                .any(|other| other.key() == plugin.key())
            {
                return Err(StateError::DuplicatePlugin(plugin.key().to_owned()));
            }
        }
        Ok(Self {
            schema,
            plugins,
            clock,
        })
    }
}

/// An immutable editor state. Applying a [`Transaction`] produces a new one;
/// unchanged parts (document subtrees, plugin values) are shared.
#[derive(Clone)]
pub struct EditorState {
    config: Arc<Configuration>,
    doc: Node,
    selection: Selection,
    stored_marks: Option<MarkSet>,
    scroll_to_selection: u64,
    plugin_states: Vec<Option<PluginValue>>,
}

impl fmt::Debug for EditorState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditorState")
            .field("doc", &self.doc)
            .field("selection", &self.selection)
            .field("stored_marks", &self.stored_marks)
            .finish()
    }
}

/// Result of [`EditorState::apply_transaction`].
#[derive(Debug)]
pub struct AppliedTransaction {
    pub state: EditorState,
    /// The root transaction followed by any appended ones; empty when filtered.
    pub transactions: Vec<Transaction>,
}

impl EditorState {
    pub fn create(config: EditorStateConfig) -> Result<Self, StateError> {
        let schema = match (&config.doc, &config.schema) {
            (Some(doc), _) => doc.node_type().schema().clone(),
            (None, Some(schema)) => schema.clone(),
            (None, None) => return Err(StateError::MissingSchema),
        };
        let doc = match config.doc {
            Some(doc) => doc,
            None => schema
                .top_node_type()
                .create_and_fill(None, Default::default(), MarkSet::none())
                .ok_or_else(|| {
                    StateError::Model(ModelError::InvalidContent(
                        schema.top_node_type().name().into(),
                    ))
                })?,
        };
        let selection = config
            .selection
            .unwrap_or_else(|| Selection::at_start(&doc));
        let clock = config.clock.unwrap_or_else(|| Arc::new(SystemClock));
        let configuration = Arc::new(Configuration::new(schema, config.plugins, clock)?);
        let mut state = EditorState {
            plugin_states: vec![None; configuration.plugins.len()],
            config: configuration,
            doc,
            selection,
            stored_marks: config.stored_marks,
            scroll_to_selection: 0,
        };
        state.init_plugins(None);
        Ok(state)
    }

    fn init_plugins(&mut self, previous: Option<&EditorState>) {
        for index in 0..self.config.plugins.len() {
            let plugin = self.config.plugins[index].clone();
            let kept = previous.and_then(|previous| previous.plugin_state_by_key(plugin.key()));
            self.plugin_states[index] = kept.or_else(|| plugin.init(self));
        }
    }

    pub fn schema(&self) -> &Schema {
        &self.config.schema
    }

    pub fn doc(&self) -> &Node {
        &self.doc
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Marks to apply to the next typed text, when set explicitly.
    pub fn stored_marks(&self) -> Option<&MarkSet> {
        self.stored_marks.as_ref()
    }

    /// Increments every time a transaction asks to scroll the selection into view.
    pub fn scroll_to_selection(&self) -> u64 {
        self.scroll_to_selection
    }

    pub fn plugins(&self) -> &[Arc<dyn Plugin>] {
        &self.config.plugins
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.config.clock
    }

    pub fn plugin(&self, key: &str) -> Option<&Arc<dyn Plugin>> {
        self.config
            .plugins
            .iter()
            .find(|plugin| plugin.key() == key)
    }

    fn plugin_state_by_key(&self, key: &str) -> Option<PluginValue> {
        let index = self
            .config
            .plugins
            .iter()
            .position(|plugin| plugin.key() == key)?;
        self.plugin_states[index].clone()
    }

    /// The state of the plugin with `key`, downcast to `T`.
    pub fn plugin_state<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> {
        self.plugin_state_by_key(key)?.downcast::<T>().ok()
    }

    /// A new transaction timestamped by this state's clock.
    pub fn tr(&self) -> Transaction {
        Transaction::new(self, self.config.clock.now_ms())
    }

    pub fn apply(&self, tr: Transaction) -> Result<EditorState, StateError> {
        Ok(self.apply_transaction(tr)?.state)
    }

    fn filter_transaction(&self, tr: &Transaction, ignore: Option<usize>) -> bool {
        self.config
            .plugins
            .iter()
            .enumerate()
            .all(|(index, plugin)| Some(index) == ignore || plugin.filter_transaction(tr, self))
    }

    /// Applies `root` and lets plugins append follow-up transactions until
    /// none respond, giving each plugin only the transactions it has not seen.
    pub fn apply_transaction(&self, root: Transaction) -> Result<AppliedTransaction, StateError> {
        if !self.filter_transaction(&root, None) {
            return Ok(AppliedTransaction {
                state: self.clone(),
                transactions: Vec::new(),
            });
        }
        let mut new_state = self.apply_inner(&root)?;
        let mut root_ref: Option<Arc<Transaction>> = None;
        let mut transactions = vec![root];
        // Per plugin: the state it last saw and how many transactions.
        let mut seen: Option<Vec<(EditorState, usize)>> = None;
        loop {
            let mut have_new = false;
            for index in 0..self.config.plugins.len() {
                let plugin = self.config.plugins[index].clone();
                let (n, old_state) = match &seen {
                    Some(seen) => (seen[index].1, seen[index].0.clone()),
                    None => (0, self.clone()),
                };
                let appended = if n < transactions.len() {
                    plugin.append_transaction(&transactions[n..], &old_state, &new_state)
                } else {
                    None
                };
                if let Some(mut tr) = appended
                    && new_state.filter_transaction(&tr, Some(index))
                {
                    let root = root_ref
                        .get_or_insert_with(|| Arc::new(transactions[0].clone()))
                        .clone();
                    tr.set_meta(meta::APPENDED_TRANSACTION, root);
                    if seen.is_none() {
                        seen = Some(
                            (0..self.config.plugins.len())
                                .map(|j| {
                                    if j < index {
                                        (new_state.clone(), transactions.len())
                                    } else {
                                        (self.clone(), 0)
                                    }
                                })
                                .collect(),
                        );
                    }
                    new_state = new_state.apply_inner(&tr)?;
                    transactions.push(tr);
                    have_new = true;
                }
                if let Some(seen) = &mut seen {
                    seen[index] = (new_state.clone(), transactions.len());
                }
            }
            if !have_new {
                return Ok(AppliedTransaction {
                    state: new_state,
                    transactions,
                });
            }
        }
    }

    fn apply_inner(&self, tr: &Transaction) -> Result<EditorState, StateError> {
        if tr.before() != &self.doc {
            return Err(StateError::MismatchedTransaction);
        }
        let selection = tr.selection();
        let stored_marks = if selection.cursor().is_some() {
            tr.stored_marks().cloned()
        } else {
            None
        };
        let mut new_state = EditorState {
            config: self.config.clone(),
            doc: tr.doc.clone(),
            selection,
            stored_marks,
            scroll_to_selection: self.scroll_to_selection + u64::from(tr.scrolled_into_view()),
            plugin_states: self.plugin_states.clone(),
        };
        for index in 0..self.config.plugins.len() {
            if let Some(value) = &self.plugin_states[index] {
                let next = self.config.plugins[index].apply(tr, value, self, &new_state);
                new_state.plugin_states[index] = Some(next);
            }
        }
        Ok(new_state)
    }

    /// The same document and selection with a different plugin set.
    /// Plugins present in both keep their state.
    pub fn reconfigure(&self, plugins: Vec<Arc<dyn Plugin>>) -> Result<EditorState, StateError> {
        let config = Arc::new(Configuration::new(
            self.config.schema.clone(),
            plugins,
            self.config.clock.clone(),
        )?);
        let mut state = EditorState {
            plugin_states: vec![None; config.plugins.len()],
            config,
            doc: self.doc.clone(),
            selection: self.selection.clone(),
            stored_marks: self.stored_marks.clone(),
            scroll_to_selection: self.scroll_to_selection,
        };
        state.init_plugins(Some(self));
        Ok(state)
    }

    /// `{doc, selection, storedMarks?}` plus the states of the plugins
    /// named in `plugin_fields` (`(json field, plugin key)`).
    pub fn to_json(&self, plugin_fields: &[(&str, &str)]) -> Result<Value, StateError> {
        let mut object = Map::new();
        object.insert("doc".into(), self.doc.to_json());
        object.insert("selection".into(), self.selection.to_json());
        if let Some(marks) = &self.stored_marks {
            object.insert(
                "storedMarks".into(),
                Value::Array(marks.iter().map(|m| m.to_json()).collect()),
            );
        }
        for (field, key) in plugin_fields {
            if *field == "doc" || *field == "selection" {
                return Err(StateError::ReservedField((*field).to_owned()));
            }
            if let (Some(plugin), Some(value)) = (self.plugin(key), self.plugin_state_by_key(key))
                && let Some(json) = plugin.state_to_json(&value)
            {
                object.insert((*field).to_owned(), json);
            }
        }
        Ok(Value::Object(object))
    }

    pub fn from_json(
        schema: &Schema,
        plugins: Vec<Arc<dyn Plugin>>,
        value: &Value,
        plugin_fields: &[(&str, &str)],
    ) -> Result<EditorState, StateError> {
        let doc = schema.node_from_json(value.get("doc").unwrap_or(&Value::Null))?;
        let selection = Selection::from_json(&doc, value.get("selection").unwrap_or(&Value::Null))?;
        let stored_marks = match value.get("storedMarks") {
            Some(Value::Array(marks)) => Some(MarkSet::from_marks(
                marks
                    .iter()
                    .map(|m| schema.mark_from_json(m))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            _ => None,
        };
        let configuration = Arc::new(Configuration::new(
            schema.clone(),
            plugins,
            Arc::new(SystemClock),
        )?);
        let mut state = EditorState {
            plugin_states: vec![None; configuration.plugins.len()],
            config: configuration,
            doc,
            selection,
            stored_marks,
            scroll_to_selection: 0,
        };
        for index in 0..state.config.plugins.len() {
            let plugin = state.config.plugins[index].clone();
            let restored = plugin_fields
                .iter()
                .find(|(_, key)| *key == plugin.key())
                .and_then(|(field, _)| value.get(*field))
                .and_then(|json| plugin.state_from_json(json, &state));
            state.plugin_states[index] = restored.or_else(|| plugin.init(&state));
        }
        Ok(state)
    }
}
