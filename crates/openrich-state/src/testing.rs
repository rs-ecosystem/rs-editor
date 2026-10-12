//! `TestState`, a mutable wrapper for state tests (like ProseMirror's test helper).

use std::sync::Arc;

use openrich_model::Node;
use openrich_model::testing::TaggedNode;

use crate::{
    EditorState, EditorStateConfig, ManualClock, NodeSelection, Plugin, Selection, TextSelection,
    Transaction,
};

/// The selection a tagged test document asks for: `<a>`..`<b>` as text,
/// `<a>` before a node as a node selection, else the document start.
pub fn sel_for(tagged: &TaggedNode) -> Selection {
    let doc = &tagged.node;
    match tagged.tags.get("a") {
        Some(&a) => {
            let ra = doc.resolve(a).expect("valid <a>");
            if ra.parent().inline_content() {
                let head = tagged.tags.get("b").copied().unwrap_or(a);
                TextSelection::create(doc, a, head).expect("valid <b>")
            } else {
                NodeSelection::create(doc, a).expect("node after <a>")
            }
        }
        None => Selection::at_start(doc),
    }
}

/// An editor state that tests can mutate in place.
#[derive(Debug)]
pub struct TestState {
    pub state: EditorState,
    pub clock: Arc<ManualClock>,
}

impl TestState {
    pub fn new(tagged: &TaggedNode) -> Self {
        Self::with_plugins(tagged, Vec::new())
    }

    pub fn with_plugins(tagged: &TaggedNode, plugins: Vec<Arc<dyn Plugin>>) -> Self {
        let clock = ManualClock::new(0);
        let state = EditorState::create(EditorStateConfig {
            doc: Some(tagged.node.clone()),
            selection: Some(sel_for(tagged)),
            plugins,
            clock: Some(clock.clone()),
            ..Default::default()
        })
        .expect("valid test state");
        Self { state, clock }
    }

    pub fn tr(&self) -> Transaction {
        self.state.tr()
    }

    pub fn apply(&mut self, tr: Transaction) {
        self.state = self.state.apply(tr).expect("transaction applies");
    }

    /// Runs a command, applying what it dispatches. Returns whether it ran.
    pub fn command(
        &mut self,
        command: impl FnOnce(&EditorState, Option<&mut dyn FnMut(Transaction)>) -> bool,
    ) -> bool {
        let mut produced = Vec::new();
        let applied = command(&self.state, Some(&mut |tr| produced.push(tr)));
        for tr in produced {
            self.apply(tr);
        }
        applied
    }

    pub fn type_text(&mut self, text: &str) {
        let mut tr = self.tr();
        tr.insert_text(text).expect("insert text");
        self.apply(tr);
    }

    pub fn delete_selection(&mut self) {
        let mut tr = self.tr();
        tr.delete_selection().expect("delete selection");
        self.apply(tr);
    }

    pub fn text_sel(&mut self, anchor: usize, head: usize) {
        let selection =
            TextSelection::create(self.doc(), anchor, head).expect("valid text selection");
        let mut tr = self.tr();
        tr.set_selection(selection);
        self.apply(tr);
    }

    pub fn node_sel(&mut self, pos: usize) {
        let selection = NodeSelection::create(self.doc(), pos).expect("valid node selection");
        let mut tr = self.tr();
        tr.set_selection(selection);
        self.apply(tr);
    }

    pub fn doc(&self) -> &Node {
        self.state.doc()
    }

    pub fn selection(&self) -> &Selection {
        self.state.selection()
    }
}
