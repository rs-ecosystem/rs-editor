//! Ports of prosemirror-state's `test-selection.ts` and `test-state.ts`.

use std::any::Any;
use std::sync::Arc;

use rs_editor_model::testing::*;
use rs_editor_model::{Fragment, MarkSet, Node, v};
use serde_json::{Value, json};

use crate::testing::TestState;
use crate::*;

fn node(name: &str) -> Node {
    schema()
        .node_type(name)
        .unwrap()
        .create(None, Fragment::empty(), MarkSet::none())
        .unwrap()
}

fn text(value: &str) -> Node {
    schema().text(value, MarkSet::none()).unwrap()
}

fn empty_state() -> EditorState {
    EditorState::create(EditorStateConfig {
        schema: Some(schema()),
        ..Default::default()
    })
    .unwrap()
}

// ---- Selection ---------------------------------------------------------------

#[test]
fn selection_follows_changes() {
    let mut state = TestState::new(&doc(v![p(v!["hi"])]));
    let mut tr = state.tr();
    tr.insert_text_at("xy", 1, 1).unwrap();
    state.apply(tr);
    assert_eq!(
        (state.selection().head(), state.selection().anchor()),
        (3, 3)
    );
    let mut tr = state.tr();
    tr.insert_text_at("zq", 1, 1).unwrap();
    state.apply(tr);
    assert_eq!(
        (state.selection().head(), state.selection().anchor()),
        (5, 5)
    );
    let mut tr = state.tr();
    tr.insert_text_at("uv", 7, 7).unwrap();
    state.apply(tr);
    assert_eq!(
        (state.selection().head(), state.selection().anchor()),
        (5, 5)
    );
}

#[test]
fn selection_moves_after_inserted_content() {
    let mut state = TestState::new(&doc(v![p(v!["hi"])]));
    state.text_sel(2, 3);
    state.type_text("o");
    assert_eq!(
        (state.selection().head(), state.selection().anchor()),
        (3, 3)
    );
}

#[test]
fn selection_moves_after_an_inserted_leaf_node() {
    let mut state = TestState::new(&doc(v![p(v!["foobar"])]));
    state.text_sel(4, 4);
    let mut tr = state.tr();
    tr.replace_selection_with(node("horizontal_rule"), true)
        .unwrap();
    state.apply(tr);
    assert_eq!(state.doc(), &doc(v![p(v!["foo"]), hr(), p(v!["bar"])]).node);
    assert_eq!(state.selection().head(), 7);
    state.text_sel(10, 10);
    let mut tr = state.tr();
    tr.replace_selection_with(node("horizontal_rule"), true)
        .unwrap();
    state.apply(tr);
    assert_eq!(
        state.doc(),
        &doc(v![p(v!["foo"]), hr(), p(v!["bar"]), hr()]).node
    );
    assert_eq!(state.selection().from(), 11);
}

#[test]
fn selection_allows_typing_over_a_leaf_node() {
    let mut state = TestState::new(&doc(v![p(v!["a"]), "<a>", hr(), p(v!["b"])]));
    state.node_sel(3);
    let mut tr = state.tr();
    tr.replace_selection_with(text("x"), true).unwrap();
    state.apply(tr);
    assert_eq!(
        state.doc(),
        &doc(v![p(v!["a"]), p(v!["x"]), p(v!["b"])]).node
    );
    assert_eq!(
        (state.selection().head(), state.selection().anchor()),
        (5, 5)
    );
}

#[test]
fn selection_allows_deleting_a_selected_block() {
    let mut state = TestState::new(&doc(v![
        p(v!["foo"]),
        ul(v![
            li(v![p(v!["bar"])]),
            li(v![p(v!["baz"])]),
            li(v![p(v!["quux"])])
        ])
    ]));
    state.node_sel(0);
    state.delete_selection();
    assert_eq!(
        state.doc(),
        &doc(v![ul(v![
            li(v![p(v!["bar"])]),
            li(v![p(v!["baz"])]),
            li(v![p(v!["quux"])])
        ])])
        .node
    );
    assert_eq!(state.selection().head(), 3);
    state.node_sel(2);
    state.delete_selection();
    assert_eq!(
        state.doc(),
        &doc(v![ul(v![li(v![p(v!["baz"])]), li(v![p(v!["quux"])])])]).node
    );
    assert_eq!(state.selection().head(), 3);
    state.node_sel(9);
    state.delete_selection();
    assert_eq!(state.doc(), &doc(v![ul(v![li(v![p(v!["baz"])])])]).node);
    assert_eq!(state.selection().head(), 6);
    state.node_sel(0);
    state.delete_selection();
    assert_eq!(state.doc(), &doc(v![p(v![])]).node);
}

fn stored_mark_names(state: &TestState) -> Option<Vec<String>> {
    state.state.stored_marks().map(|marks| {
        marks
            .iter()
            .map(|m| m.mark_type().name().to_owned())
            .collect()
    })
}

#[test]
fn selection_preserves_the_marks_of_a_deleted_selection() {
    let mut state = TestState::new(&doc(v![p(v!["foo", em(v!["<a>bar<b>"]), "baz"])]));
    state.delete_selection();
    assert_eq!(stored_mark_names(&state).map(|m| m.len()), Some(1));
}

#[test]
fn selection_does_not_preserve_non_inclusive_marks_of_a_deleted_selection() {
    let mut state = TestState::new(&doc(v![p(v!["foo", a(v![em(v!["<a>bar<b>"])]), "baz"])]));
    state.delete_selection();
    assert_eq!(stored_mark_names(&state).map(|m| m.len()), Some(1));
}

#[test]
fn selection_does_not_preserve_marks_when_deleting_at_the_end_of_a_block() {
    let mut state = TestState::new(&doc(v![p(v!["foo", em(v!["bar<a>"])]), p(v!["b<b>az"])]));
    state.delete_selection();
    assert_eq!(stored_mark_names(&state), None);
}

#[test]
fn selection_drops_non_inclusive_marks_at_the_end_of_a_deleted_span() {
    let mut state = TestState::new(&doc(v![p(v![
        "foo",
        a(v!["ba", em(v!["<a>r<b>"])]),
        "baz"
    ])]));
    state.delete_selection();
    assert_eq!(stored_mark_names(&state), Some(vec!["em".to_owned()]));
}

#[test]
fn selection_keeps_non_inclusive_marks_when_still_inside_them() {
    let mut state = TestState::new(&doc(v![p(v![
        "foo",
        a(v!["b", em(v!["<a>a<b>"]), "r"]),
        "baz"
    ])]));
    state.delete_selection();
    assert_eq!(stored_mark_names(&state).map(|m| m.len()), Some(2));
}

#[test]
fn selection_preserves_marks_when_typing_over_marked_text() {
    let mut state = TestState::new(&doc(v![p(v!["foo ", em(v!["<a>bar<b>"]), " baz"])]));
    state.type_text("quux");
    assert_eq!(
        state.doc(),
        &doc(v![p(v!["foo ", em(v!["quux"]), " baz"])]).node
    );
    let mut tr = state.tr();
    tr.insert_text_at("bar", 5, 9).unwrap();
    state.apply(tr);
    assert_eq!(
        state.doc(),
        &doc(v![p(v!["foo ", em(v!["bar"]), " baz"])]).node
    );
}

#[test]
fn selection_allows_deleting_a_leaf() {
    let mut state = TestState::new(&doc(v![p(v!["a"]), hr(), hr(), p(v!["b"])]));
    state.node_sel(3);
    state.delete_selection();
    assert_eq!(state.doc(), &doc(v![p(v!["a"]), hr(), p(v!["b"])]).node);
    assert_eq!(state.selection().from(), 3);
    state.delete_selection();
    assert_eq!(state.doc(), &doc(v![p(v!["a"]), p(v!["b"])]).node);
    assert_eq!(state.selection().head(), 4);
}

#[test]
fn selection_properly_handles_deleting_the_selection() {
    let mut state = TestState::new(&doc(v![
        p(v!["foo", img(), "bar"]),
        blockquote(v![p(v!["hi"])]),
        p(v!["ay"])
    ]));
    state.node_sel(4);
    state.delete_selection();
    assert_eq!(
        state.doc(),
        &doc(v![
            p(v!["foobar"]),
            blockquote(v![p(v!["hi"])]),
            p(v!["ay"])
        ])
        .node
    );
    assert_eq!(state.selection().head(), 4);
    state.node_sel(9);
    state.delete_selection();
    assert_eq!(state.doc(), &doc(v![p(v!["foobar"]), p(v!["ay"])]).node);
    assert_eq!(state.selection().from(), 9);
    state.node_sel(8);
    state.delete_selection();
    assert_eq!(state.doc(), &doc(v![p(v!["foobar"])]).node);
    assert_eq!(state.selection().from(), 7);
}

#[test]
fn selection_can_replace_inline_selections() {
    let mut state = TestState::new(&doc(v![p(v!["foo", img(), "bar", img(), "baz"])]));
    state.node_sel(4);
    let mut tr = state.tr();
    tr.replace_selection_with(node("hard_break"), true).unwrap();
    state.apply(tr);
    assert_eq!(
        state.doc(),
        &doc(v![p(v!["foo", br(), "bar", img(), "baz"])]).node
    );
    assert_eq!(state.selection().head(), 5);
    assert!(state.selection().empty());
    state.node_sel(8);
    state.type_text("abc");
    assert_eq!(state.doc(), &doc(v![p(v!["foo", br(), "barabcbaz"])]).node);
    assert_eq!(state.selection().head(), 11);
    assert!(state.selection().empty());
    state.node_sel(0);
    state.type_text("xyz");
    assert_eq!(state.doc(), &doc(v![p(v!["xyz"])]).node);
}

#[test]
fn selection_can_replace_a_block_selection() {
    let mut state = TestState::new(&doc(v![
        p(v!["abc"]),
        hr(),
        hr(),
        blockquote(v![p(v!["ow"])])
    ]));
    state.node_sel(5);
    let mut tr = state.tr();
    tr.replace_selection_with(node("code_block"), true).unwrap();
    state.apply(tr);
    assert_eq!(
        state.doc(),
        &doc(v![
            p(v!["abc"]),
            pre(v![]),
            hr(),
            blockquote(v![p(v!["ow"])])
        ])
        .node
    );
    assert_eq!(state.selection().from(), 7);
    state.node_sel(8);
    let mut tr = state.tr();
    tr.replace_selection_with(node("paragraph"), true).unwrap();
    state.apply(tr);
    assert_eq!(
        state.doc(),
        &doc(v![p(v!["abc"]), pre(v![]), hr(), p(v![])]).node
    );
    assert_eq!(state.selection().from(), 9);
}

#[test]
fn selection_puts_the_cursor_after_an_inserted_list_item() {
    let mut state = TestState::new(&doc(v![p(v!["<a>abc"])]));
    let source = doc(v![ul(v![li(v![p(v!["<a>def<b>"])])])]);
    let mut tr = state.tr();
    tr.replace_selection(
        source
            .node
            .slice(source.tag("a"), source.tag("b"), true)
            .unwrap(),
    )
    .unwrap();
    state.apply(tr);
    assert_eq!(state.selection().from(), 6);
}

#[test]
fn text_selection_between_uses_arguments_when_possible() {
    let d = doc(v![p(v!["f<a>o<b>o"])]);
    let s = TextSelection::between(
        &d.node.resolve(d.tag("b")).unwrap(),
        &d.node.resolve(d.tag("a")).unwrap(),
        None,
    );
    assert_eq!((s.anchor(), s.head()), (d.tag("b"), d.tag("a")));
}

#[test]
fn text_selection_between_adjusts_when_necessary() {
    let d = doc(v!["<a>", p(v!["foo"])]);
    let pos = d.node.resolve(d.tag("a")).unwrap();
    assert_eq!(TextSelection::between(&pos, &pos, None).anchor(), 1);
}

#[test]
fn text_selection_between_uses_bias_when_adjusting() {
    let d = doc(v![p(v!["foo"]), "<a>", p(v!["bar"])]);
    let pos = d.node.resolve(d.tag("a")).unwrap();
    assert_eq!(TextSelection::between(&pos, &pos, Some(-1)).anchor(), 4);
    assert_eq!(TextSelection::between(&pos, &pos, Some(1)).anchor(), 6);
}

#[test]
fn text_selection_between_falls_back_to_a_node_selection() {
    let d = doc(v![hr(), "<a>"]);
    let pos = d.node.resolve(d.tag("a")).unwrap();
    let s = TextSelection::between(&pos, &pos, None);
    assert_eq!(
        s.as_node().map(|s| s.node().clone()).as_ref(),
        d.node.first_child()
    );
}

#[test]
fn text_selection_between_collapses_towards_the_other_argument() {
    let d = doc(v!["<a>", p(v!["foo"]), "<b>"]);
    let (ra, rb) = (
        d.node.resolve(d.tag("a")).unwrap(),
        d.node.resolve(d.tag("b")).unwrap(),
    );
    let s = TextSelection::between(&ra, &rb, None);
    assert_eq!((s.anchor(), s.head()), (1, 4));
    let s = TextSelection::between(&rb, &ra, None);
    assert_eq!((s.anchor(), s.head()), (4, 1));
}

#[test]
fn bookmarks_survive_mapping() {
    let d = doc(v![p(v!["ab<a>cd<b>ef"])]);
    let state = TestState::new(&d);
    let bookmark = state.selection().bookmark();
    let mut tr = state.tr();
    tr.insert_text_at("XY", 1, 1).unwrap();
    let mapped = bookmark.map(&tr.mapping).resolve(&tr.doc);
    assert_eq!((mapped.anchor(), mapped.head()), (5, 7));
}

// ---- State -------------------------------------------------------------------

struct MessageCount;

impl Plugin for MessageCount {
    fn key(&self) -> &str {
        "messageCount"
    }

    fn init(&self, _state: &EditorState) -> Option<PluginValue> {
        Some(Arc::new(0u64))
    }

    fn apply(
        &self,
        _tr: &Transaction,
        value: &PluginValue,
        _old: &EditorState,
        _new: &EditorState,
    ) -> PluginValue {
        Arc::new(value.downcast_ref::<u64>().copied().unwrap_or(0) + 1)
    }

    fn state_to_json(&self, value: &PluginValue) -> Option<Value> {
        value.downcast_ref::<u64>().map(|count| json!(count))
    }

    fn state_from_json(&self, value: &Value, _state: &EditorState) -> Option<PluginValue> {
        value.as_u64().map(|count| Arc::new(count) as PluginValue)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct Filtering;

impl Plugin for Filtering {
    fn key(&self) -> &str {
        "transactionPlugin"
    }

    fn filter_transaction(&self, tr: &Transaction, _state: &EditorState) -> bool {
        tr.get_meta::<bool>("filtered") != Some(&true)
    }

    fn append_transaction(
        &self,
        trs: &[Transaction],
        _old: &EditorState,
        new: &EditorState,
    ) -> Option<Transaction> {
        let last = trs.last()?;
        if last.get_meta::<bool>("append") == Some(&true) {
            let mut tr = new.tr();
            tr.insert_text("A").ok()?;
            Some(tr)
        } else {
            None
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn count(state: &EditorState) -> Option<u64> {
    state
        .plugin_state::<u64>("messageCount")
        .map(|value| *value)
}

#[test]
fn state_creates_a_default_doc_and_selection() {
    assert_eq!(empty_state().doc(), &doc(v![p(v![])]).node);
    let state = EditorState::create(EditorStateConfig {
        doc: Some(doc(v![p(v!["foo"])]).node),
        ..Default::default()
    })
    .unwrap();
    assert_eq!((state.selection().from(), state.selection().to()), (1, 1));
}

#[test]
fn state_applies_transform_transactions() {
    let state = empty_state();
    let mut tr = state.tr();
    tr.insert_text("hi").unwrap();
    let new_state = state.apply(tr).unwrap();
    assert_eq!(state.doc(), &doc(v![p(v![])]).node);
    assert_eq!(new_state.doc(), &doc(v![p(v!["hi"])]).node);
    assert_eq!(new_state.selection().from(), 3);
}

#[test]
fn state_supports_plugin_fields() {
    let state = EditorState::create(EditorStateConfig {
        schema: Some(schema()),
        plugins: vec![Arc::new(MessageCount)],
        ..Default::default()
    })
    .unwrap();
    let new_state = state.apply(state.tr()).unwrap().apply(state.tr()).unwrap();
    assert_eq!(count(&state), Some(0));
    assert_eq!(count(&new_state), Some(2));
}

#[test]
fn state_round_trips_through_json() {
    let state = EditorState::create(EditorStateConfig {
        doc: Some(doc(v![p(v!["ok"])]).node),
        plugins: vec![Arc::new(MessageCount)],
        ..Default::default()
    })
    .unwrap();
    let mut tr = state.tr();
    tr.set_selection(TextSelection::create(state.doc(), 3, 3).unwrap());
    let state = state.apply(tr).unwrap();
    let fields = [("count", "messageCount")];
    let json = state.to_json(&fields).unwrap();
    assert_eq!(
        json,
        json!({
            "doc": {"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "ok"}]}]},
            "selection": {"type": "text", "anchor": 3, "head": 3},
            "count": 1
        })
    );
    let copy =
        EditorState::from_json(&schema(), vec![Arc::new(MessageCount)], &json, &fields).unwrap();
    assert_eq!(copy.doc(), state.doc());
    assert_eq!(copy.selection().from(), 3);
    assert_eq!(count(&copy), Some(1));

    let limited = state.to_json(&[]).unwrap();
    assert!(limited.get("count").is_none());
    let restored =
        EditorState::from_json(&schema(), vec![Arc::new(MessageCount)], &limited, &[]).unwrap();
    assert_eq!(count(&restored), Some(0));
}

#[test]
fn state_persists_stored_marks() {
    let em = schema().mark_type("em").unwrap().create(None).unwrap();
    let state = EditorState::create(EditorStateConfig {
        doc: Some(doc(v![p(v!["ok"])]).node),
        stored_marks: Some(MarkSet::from_marks([em])),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(state.stored_marks().map(|m| m.len()), Some(1));
    let copy =
        EditorState::from_json(&schema(), Vec::new(), &state.to_json(&[]).unwrap(), &[]).unwrap();
    assert_eq!(copy.stored_marks().map(|m| m.len()), Some(1));
}

#[test]
fn state_supports_reconfiguration() {
    let state = EditorState::create(EditorStateConfig {
        schema: Some(schema()),
        plugins: vec![Arc::new(MessageCount)],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(count(&state), Some(0));
    let without = state.reconfigure(Vec::new()).unwrap();
    assert_eq!(count(&without), None);
    assert!(without.plugins().is_empty());
    assert_eq!(without.doc(), &doc(v![p(v![])]).node);
    let re_add = without.reconfigure(vec![Arc::new(MessageCount)]).unwrap();
    assert_eq!(count(&re_add), Some(0));
    assert_eq!(re_add.plugins().len(), 1);
}

#[test]
fn state_rejects_duplicate_plugin_keys() {
    let result = EditorState::create(EditorStateConfig {
        schema: Some(schema()),
        plugins: vec![Arc::new(MessageCount), Arc::new(MessageCount)],
        ..Default::default()
    });
    assert_eq!(
        result.unwrap_err(),
        StateError::DuplicatePlugin("messageCount".into())
    );
}

fn with_filter() -> EditorState {
    EditorState::create(EditorStateConfig {
        schema: Some(schema()),
        plugins: vec![Arc::new(Filtering)],
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn plugins_can_filter_transactions() {
    let state = with_filter();
    let mut tr = state.tr();
    tr.insert_text("X").unwrap();
    let applied = state.apply_transaction(tr).unwrap();
    assert_eq!(applied.state.doc(), &doc(v![p(v!["X"])]).node);
    assert_eq!(applied.transactions.len(), 1);
    let mut tr = state.tr();
    tr.insert_text("Y").unwrap();
    tr.set_meta("filtered", true);
    let applied = state.apply_transaction(tr).unwrap();
    assert_eq!(applied.state.doc(), state.doc());
    assert!(applied.transactions.is_empty());
}

#[test]
fn plugins_can_append_transactions_linked_to_the_root() {
    let state = with_filter();
    let mut tr = state.tr();
    tr.insert_text("X").unwrap();
    tr.set_meta("append", true);
    let applied = state.apply_transaction(tr).unwrap();
    assert_eq!(applied.state.doc(), &doc(v![p(v!["XA"])]).node);
    assert_eq!(applied.transactions.len(), 2);
    assert_eq!(
        applied.transactions[1].appended_to().map(|root| root.id()),
        Some(applied.transactions[0].id())
    );
}

#[test]
fn applying_a_transaction_for_another_document_fails() {
    let state = empty_state();
    let other = EditorState::create(EditorStateConfig {
        doc: Some(doc(v![p(v!["x"])]).node),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        state.apply(other.tr()).unwrap_err(),
        StateError::MismatchedTransaction
    );
}

#[test]
fn transactions_use_the_injected_clock() {
    let state = TestState::new(&doc(v![p(v!["x"])]));
    state.clock.set(1234);
    assert_eq!(state.tr().time, 1234);
}
