//! Plugins (`plugin.ts`) and the clock used to timestamp transactions.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use crate::state::EditorState;
use crate::transaction::Transaction;

/// A plugin's state value. Shared between state versions, so it is cheap
/// to keep when a transaction does not change it.
pub type PluginValue = Arc<dyn Any + Send + Sync>;

/// Extends the editor state. All methods have no-op defaults; implement
/// the ones the plugin needs. View-level hooks (key handling, decorations)
/// live in the view layer and are reached through [`Plugin::as_any`].
pub trait Plugin: Send + Sync + 'static {
    /// Unique name within a state; also the key of the plugin's state.
    fn key(&self) -> &str;

    /// Initial plugin state, or `None` for a stateless plugin.
    fn init(&self, _state: &EditorState) -> Option<PluginValue> {
        None
    }

    /// The plugin state after `tr`. `new_state` already has the new
    /// document, selection and the states of earlier plugins.
    fn apply(
        &self,
        _tr: &Transaction,
        value: &PluginValue,
        _old: &EditorState,
        _new: &EditorState,
    ) -> PluginValue {
        value.clone()
    }

    /// Returning `false` drops the transaction.
    fn filter_transaction(&self, _tr: &Transaction, _state: &EditorState) -> bool {
        true
    }

    /// A follow-up transaction for transactions this plugin has not seen yet.
    fn append_transaction(
        &self,
        _transactions: &[Transaction],
        _old: &EditorState,
        _new: &EditorState,
    ) -> Option<Transaction> {
        None
    }

    fn state_to_json(&self, _value: &PluginValue) -> Option<Value> {
        None
    }

    fn state_from_json(&self, _value: &Value, _state: &EditorState) -> Option<PluginValue> {
        None
    }

    fn as_any(&self) -> &dyn Any;
}

/// Milliseconds used to timestamp transactions (undo grouping, input
/// timing). Injectable so tests and replays are deterministic.
pub trait Clock: Send + Sync + 'static {
    fn now_ms(&self) -> u64;
}

/// Wall-clock time.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64)
    }
}

/// A clock advanced by hand.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn new(start: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(start)))
    }

    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::Relaxed);
    }

    pub fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::Relaxed);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}
