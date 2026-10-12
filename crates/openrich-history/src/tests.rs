//! Port of prosemirror-history's `test-history.ts`.

use std::any::Any;
use std::sync::Arc;

use openrich_model::testing::*;
use openrich_model::{Fragment, MarkSet, Node, Slice, v};
use openrich_state::{
    EditorState, EditorStateConfig, ManualClock, Plugin, TextSelection, Transaction, meta,
};
use openrich_transform::{ReplaceStep, Step};

use super::*;

struct Env {
    clock: Arc<ManualClock>,
}

impl Env {
    fn new() -> Self {
        // History uses time 0 as "no previous event", so start later.
        Self {
            clock: ManualClock::new(10_000),
        }
    }

    fn state(
        &self,
        doc: Option<Node>,
        options: HistoryOptions,
        extra: Vec<Arc<dyn Plugin>>,
    ) -> EditorState {
        let mut plugins: Vec<Arc<dyn Plugin>> = vec![History::new(options)];
        plugins.extend(extra);
        EditorState::create(EditorStateConfig {
            schema: Some(schema()),
            doc,
            plugins,
            clock: Some(self.clock.clone()),
            ..Default::default()
        })
        .unwrap()
    }

    fn default_state(&self) -> EditorState {
        self.state(None, HistoryOptions::default(), Vec::new())
    }
}

fn type_text(state: &EditorState, text: &str) -> EditorState {
    let mut tr = state.tr();
    tr.insert_text(text).unwrap();
    state.apply(tr).unwrap()
}

fn apply(state: &EditorState, f: impl FnOnce(&mut Transaction)) -> EditorState {
    let mut tr = state.tr();
    f(&mut tr);
    state.apply(tr).unwrap()
}

fn command(state: &EditorState, cmd: openrich_state::CommandFn) -> EditorState {
    let mut result = state.clone();
    cmd(state, Some(&mut |tr| result = state.apply(tr).unwrap()));
    result
}

fn close(state: &EditorState) -> EditorState {
    apply(state, |tr| {
        close_history(tr);
    })
}

fn text(value: &str) -> Node {
    schema().text(value, MarkSet::none()).unwrap()
}

fn d(children: Vec<openrich_model::testing::Item>) -> Node {
    doc(children).node
}

/// Replaces the undo branch with its compressed form.
fn compress(state: &EditorState) -> EditorState {
    let history = state.plugin_state::<HistoryState>(HISTORY_KEY).unwrap();
    let mut compressed = history.as_ref().clone();
    compressed.done = compressed.done.compress(compressed.done.items.len());
    apply(state, |tr| {
        tr.set_meta(
            HISTORY_KEY,
            HistoryMeta {
                redo: false,
                state: Arc::new(compressed),
            },
        );
    })
}

struct PreserveItems;

impl Plugin for PreserveItems {
    fn key(&self) -> &str {
        "preserveItems"
    }

    fn history_preserve_items(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn enables_undo_and_redo() {
    let env = Env::new();
    let mut state = env.default_state();
    state = type_text(&state, "a");
    state = type_text(&state, "b");
    assert_eq!(state.doc(), &d(v![p(v!["ab"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v![])]));
    state = command(&state, redo);
    assert_eq!(state.doc(), &d(v![p(v!["ab"])]));
}

#[test]
fn tracks_multiple_levels_of_history() {
    let env = Env::new();
    let mut state = env.default_state();
    state = type_text(&state, "a");
    state = type_text(&state, "b");
    state = apply(&state, |tr| {
        tr.insert_text_at("c", 1, 1).unwrap();
    });
    assert_eq!(state.doc(), &d(v![p(v!["cab"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["ab"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v![])]));
    state = command(&state, redo);
    assert_eq!(state.doc(), &d(v![p(v!["ab"])]));
    state = command(&state, redo);
    assert_eq!(state.doc(), &d(v![p(v!["cab"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["ab"])]));
}

#[test]
fn starts_a_new_event_when_new_group_delay_elapses() {
    let env = Env::new();
    let options = HistoryOptions {
        new_group_delay: 1000,
        ..Default::default()
    };
    let mut state = env.state(None, options, Vec::new());
    let typed = |state: &EditorState, text: &str, time: u64| {
        apply(state, |tr| {
            tr.insert_text(text).unwrap();
            tr.set_time(time);
        })
    };
    state = typed(&state, "a", 1000);
    state = typed(&state, "b", 1600);
    assert_eq!(undo_depth(&state), 1);
    state = typed(&state, "c", 2700);
    assert_eq!(undo_depth(&state), 2);
    state = command(&state, undo);
    state = typed(&state, "d", 2800);
    assert_eq!(undo_depth(&state), 2);
}

#[test]
fn starts_a_new_event_for_non_adjacent_changes() {
    let env = Env::new();
    let options = HistoryOptions {
        new_group_delay: 1000,
        ..Default::default()
    };
    let mut state = env.state(Some(d(v![p(v!["abc"])])), options, Vec::new());
    state = apply(&state, |tr| {
        tr.insert_text_at("x", 1, 1).unwrap();
    });
    state = apply(&state, |tr| {
        tr.insert_text_at("y", 5, 5).unwrap();
    });
    assert_eq!(undo_depth(&state), 2);
}

#[test]
fn is_not_confused_by_non_replacement_steps_when_checking_adjacency() {
    let env = Env::new();
    let options = HistoryOptions {
        new_group_delay: 1000,
        ..Default::default()
    };
    let em = schema().mark_type("em").unwrap().create(None).unwrap();
    let mut state = env.state(Some(d(v![p(v![])])), options, Vec::new());
    state = apply(&state, |tr| {
        tr.insert_text_at("x", 1, 1).unwrap();
        tr.add_mark(1, 2, &em).unwrap();
    });
    state = apply(&state, |tr| {
        tr.insert_text_at("y", 2, 2).unwrap();
        tr.add_mark(2, 3, &em).unwrap();
    });
    assert_eq!(undo_depth(&state), 1);
}

#[test]
fn allows_changes_that_are_not_part_of_the_history() {
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "hello");
    state = apply(&state, |tr| {
        tr.insert_text_at("oops", 1, 1).unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    state = apply(&state, |tr| {
        tr.insert_text_at("!", 10, 10).unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["oops!"])]));
}

#[test]
fn is_not_confused_by_an_undo_not_adding_any_redo_item() {
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "foo");
    state = apply(&state, |tr| {
        tr.replace_with(1, 4, Fragment::from_node(text("bar")))
            .unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    state = command(&state, undo);
    state = command(&state, redo);
    assert_eq!(state.doc(), &d(v![p(v!["bar"])]));
}

fn unsynced_complex(env: &Env, compress_history: bool) {
    let mut state = type_text(&env.default_state(), "hello");
    state = close(&state);
    state = type_text(&state, "!");
    state = apply(&state, |tr| {
        tr.insert_text_at("....", 1, 1).unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    state = apply(&state, |tr| {
        tr.split(3, 1, None).unwrap();
    });
    assert_eq!(state.doc(), &d(v![p(v![".."]), p(v!["..hello!"])]));
    state = apply(&state, |tr| {
        tr.split(2, 1, None).unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    if compress_history {
        state = compress(&state);
    }
    state = command(&state, undo);
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["."]), p(v!["...hello"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["."]), p(v!["..."])]));
}

#[test]
fn handles_complex_editing_sequences() {
    unsynced_complex(&Env::new(), false);
}

#[test]
fn handles_complex_editing_sequences_with_compression() {
    unsynced_complex(&Env::new(), true);
}

#[test]
fn supports_overlapping_edits() {
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "hello");
    state = close(&state);
    state = apply(&state, |tr| {
        tr.delete(1, 6).unwrap();
    });
    assert_eq!(state.doc(), &d(v![p(v![])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["hello"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v![])]));
}

#[test]
fn supports_overlapping_edits_that_are_not_collapsed() {
    let env = Env::new();
    let mut state = apply(&env.default_state(), |tr| {
        tr.insert_text_at("h", 1, 1).unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    state = type_text(&state, "ello");
    state = close(&state);
    state = apply(&state, |tr| {
        tr.delete(1, 6).unwrap();
    });
    assert_eq!(state.doc(), &d(v![p(v![])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["hello"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["h"])]));
}

#[test]
fn supports_overlapping_unsynced_deletes() {
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "hi");
    state = close(&state);
    state = type_text(&state, "hello");
    state = apply(&state, |tr| {
        tr.delete(1, 8).unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    assert_eq!(state.doc(), &d(v![p(v![])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v![])]));
}

#[test]
fn goes_back_and_forth_through_history_multiple_times() {
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "one");
    state = type_text(&state, " two");
    state = close(&state);
    state = type_text(&state, " three");
    state = apply(&state, |tr| {
        tr.insert_text_at("zero ", 1, 1).unwrap();
    });
    state = close(&state);
    state = apply(&state, |tr| {
        tr.split(1, 1, None).unwrap();
    });
    state = apply(&state, |tr| {
        let selection = TextSelection::create(&tr.doc, 1, 1).unwrap();
        tr.set_selection(selection);
    });
    state = type_text(&state, "top");
    for i in 0..6 {
        let re = i % 2 == 1;
        for _ in 0..4 {
            state = command(&state, if re { redo } else { undo });
        }
        let expected = if re {
            d(v![p(v!["top"]), p(v!["zero one two three"])])
        } else {
            d(v![p(v![])])
        };
        assert_eq!(state.doc(), &expected, "round {i}");
    }
}

#[test]
fn supports_non_tracked_changes_next_to_tracked_changes() {
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "o");
    state = apply(&state, |tr| {
        tr.split(1, 1, None).unwrap();
    });
    state = apply(&state, |tr| {
        tr.insert_text_at("zzz", 4, 4).unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["zzz"])]));
}

#[test]
fn goes_back_and_forth_when_preserving_items() {
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "one");
    state = type_text(&state, " two");
    state = close(&state);
    state = apply(&state, |tr| {
        let head = tr.selection().head();
        tr.insert_text_at("xxx", head, head).unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    state = type_text(&state, " three");
    state = apply(&state, |tr| {
        tr.insert_text_at("zero ", 1, 1).unwrap();
    });
    state = close(&state);
    state = apply(&state, |tr| {
        tr.split(1, 1, None).unwrap();
    });
    state = apply(&state, |tr| {
        let selection = TextSelection::create(&tr.doc, 1, 1).unwrap();
        tr.set_selection(selection);
    });
    state = type_text(&state, "top");
    state = apply(&state, |tr| {
        tr.insert_text_at("yyy", 1, 1).unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    for i in 0..3 {
        if i == 2 {
            state = compress(&state);
        }
        for _ in 0..4 {
            state = command(&state, undo);
        }
        assert_eq!(state.doc(), &d(v![p(v!["yyyxxx"])]), "undo round {i}");
        for _ in 0..4 {
            state = command(&state, redo);
        }
        assert_eq!(
            state.doc(),
            &d(v![p(v!["yyytop"]), p(v!["zero one twoxxx three"])]),
            "redo round {i}"
        );
    }
}

#[test]
fn restores_selection_on_undo_and_redo() {
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "hi");
    state = close(&state);
    state = apply(&state, |tr| {
        let selection = TextSelection::create(&tr.doc, 1, 3).unwrap();
        tr.set_selection(selection);
    });
    let selection = state.selection().clone();
    state = apply(&state, |tr| {
        tr.replace_with(
            selection.from(),
            selection.to(),
            Fragment::from_node(text("hello")),
        )
        .unwrap();
    });
    let selection2 = state.selection().clone();
    state = command(&state, undo);
    assert_eq!(state.selection(), &selection);
    state = command(&state, redo);
    assert_eq!(state.selection(), &selection2);
}

#[test]
fn rebases_selection_on_undo() {
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "hi");
    state = close(&state);
    state = apply(&state, |tr| {
        let selection = TextSelection::create(&tr.doc, 1, 3).unwrap();
        tr.set_selection(selection);
    });
    state = apply(&state, |tr| {
        tr.insert(1, Fragment::from_node(text("hello"))).unwrap();
    });
    state = apply(&state, |tr| {
        tr.insert(1, Fragment::from_node(text("---"))).unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    state = command(&state, undo);
    assert_eq!(state.selection().head(), 6);
}

#[test]
fn handles_change_overwriting_in_item_preserving_mode() {
    let env = Env::new();
    let mut state = env.state(
        None,
        HistoryOptions::default(),
        vec![Arc::new(PreserveItems)],
    );
    state = type_text(&state, "a");
    state = type_text(&state, "b");
    state = close(&state);
    state = apply(&state, |tr| {
        let selection = TextSelection::create(&tr.doc, 1, 3).unwrap();
        tr.set_selection(selection);
    });
    state = type_text(&state, "c");
    state = command(&state, undo);
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v![])]));
}

#[test]
fn reports_undo_and_redo_depth() {
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "a");
    assert_eq!((undo_depth(&state), redo_depth(&state)), (1, 0));
    state = apply(&state, |tr| {
        tr.insert_text_at("b", 1, 1).unwrap();
        tr.set_meta(meta::ADD_TO_HISTORY, false);
    });
    assert_eq!((undo_depth(&state), redo_depth(&state)), (1, 0));
    state = command(&state, undo);
    assert_eq!((undo_depth(&state), redo_depth(&state)), (0, 1));
    state = command(&state, redo);
    assert_eq!((undo_depth(&state), redo_depth(&state)), (1, 0));
}

#[test]
fn states_without_history_are_handled() {
    let state = EditorState::create(EditorStateConfig {
        schema: Some(schema()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!((undo_depth(&state), redo_depth(&state)), (0, 0));
    assert!(!undo(&state, None));
    assert!(!redo(&state, None));
}

#[test]
fn truncates_history() {
    let env = Env::new();
    let options = HistoryOptions {
        depth: 2,
        ..Default::default()
    };
    let mut state = env.state(None, options, Vec::new());
    for i in 1i64..40 {
        state = type_text(&state, "a");
        state = close(&state);
        assert_eq!(undo_depth(&state) as i64, (i - 2) % 21 + 2, "iteration {i}");
    }
}

#[test]
fn supports_transactions_with_multiple_steps() {
    let env = Env::new();
    let mut state = apply(&env.default_state(), |tr| {
        tr.insert_text("a").unwrap();
        tr.insert_text("b").unwrap();
    });
    state = apply(&state, |tr| {
        tr.insert_text_at("c", 1, 1).unwrap();
    });
    assert_eq!(state.doc(), &d(v![p(v!["cab"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["ab"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v![])]));
    state = command(&state, redo);
    assert_eq!(state.doc(), &d(v![p(v!["ab"])]));
    state = command(&state, redo);
    assert_eq!(state.doc(), &d(v![p(v!["cab"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["ab"])]));
}

/// Appends an insertion whenever the document reaches a given size.
struct AppendAt;

impl Plugin for AppendAt {
    fn key(&self) -> &str {
        "appendAt"
    }

    fn append_transaction(
        &self,
        _trs: &[Transaction],
        _old: &EditorState,
        new: &EditorState,
    ) -> Option<Transaction> {
        if new.doc().content().size() != 4 {
            return None;
        }
        let mut tr = new.tr();
        tr.insert(1, Fragment::from_node(text("A"))).ok()?;
        Some(tr)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Appends the text in the root transaction's `add` meta.
struct AppendMeta;

impl Plugin for AppendMeta {
    fn key(&self) -> &str {
        "appendMeta"
    }

    fn append_transaction(
        &self,
        trs: &[Transaction],
        _old: &EditorState,
        new: &EditorState,
    ) -> Option<Transaction> {
        let add = trs.first()?.get_meta::<&'static str>("add")?;
        let mut tr = new.tr();
        tr.insert(1, Fragment::from_node(text(add))).ok()?;
        Some(tr)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn combines_appended_transactions_with_the_base_event() {
    let env = Env::new();
    let mut state = env.state(
        Some(d(v![p(v!["x"])])),
        HistoryOptions::default(),
        vec![Arc::new(AppendAt)],
    );
    state = apply(&state, |tr| {
        tr.insert(2, Fragment::from_node(text("I"))).unwrap();
    });
    assert_eq!(state.doc(), &d(v![p(v!["AxI"])]));
    assert_eq!(undo_depth(&state), 1);
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["x"])]));
}

#[test]
fn includes_transactions_appended_to_undo_in_the_redo_history() {
    let env = Env::new();
    let mut state = env.state(
        Some(d(v![p(v!["x"])])),
        HistoryOptions::default(),
        vec![Arc::new(AppendMeta)],
    );
    state = apply(&state, |tr| {
        tr.insert(2, Fragment::from_node(text("I"))).unwrap();
        tr.set_meta("add", "A");
    });
    assert_eq!(state.doc(), &d(v![p(v!["AxI"])]));
    let before = state.clone();
    undo(
        &before,
        Some(&mut |mut tr| {
            tr.set_meta("add", "B");
            state = before.apply(tr).unwrap();
        }),
    );
    assert_eq!(state.doc(), &d(v![p(v!["Bx"])]));
    let before = state.clone();
    redo(
        &before,
        Some(&mut |mut tr| {
            tr.set_meta("add", "C");
            state = before.apply(tr).unwrap();
        }),
    );
    assert_eq!(state.doc(), &d(v![p(v!["CAxI"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["Bx"])]));
}

#[test]
fn does_not_close_the_history_on_appended_transactions() {
    let env = Env::new();
    let mut state = env.state(
        Some(d(v![p(v!["x"])])),
        HistoryOptions::default(),
        vec![Arc::new(AppendMeta)],
    );
    state = apply(&state, |tr| {
        tr.insert(2, Fragment::from_node(text("R"))).unwrap();
        tr.set_meta("add", "A");
    });
    state = apply(&state, |tr| {
        tr.insert(3, Fragment::from_node(text("M"))).unwrap();
    });
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["x"])]));
}

#[test]
fn supports_rebasing() {
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "base");
    state = close(&state);
    let base_doc = state.doc().clone();
    let right = Step::Replace(ReplaceStep {
        from: 5,
        to: 5,
        slice: Slice::new(Fragment::from_node(text(" right")), 0, 0),
        structure: false,
    });
    state = apply(&state, |tr| {
        tr.step(right.clone()).unwrap();
    });
    assert_eq!(state.doc(), &d(v![p(v!["base right"])]));
    assert_eq!(undo_depth(&state), 2);
    let left = Step::Replace(ReplaceStep {
        from: 1,
        to: 1,
        slice: Slice::new(Fragment::from_node(text("left ")), 0, 0),
        structure: false,
    });
    state = apply(&state, |tr| {
        tr.step(right.invert(&base_doc).unwrap()).unwrap();
        tr.step(left.clone()).unwrap();
        let mapped = right.map(&tr.mapping.slice_from(1)).unwrap();
        tr.step(mapped).unwrap();
        let last = tr.steps.len() - 1;
        tr.mapping.set_mirror(0, last);
        tr.set_meta(meta::ADD_TO_HISTORY, false);
        tr.set_meta(REBASED, 1usize);
    });
    assert_eq!(state.doc(), &d(v![p(v!["left base right"])]));
    assert_eq!(undo_depth(&state), 2);
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["left base"])]));
    state = command(&state, redo);
    assert_eq!(state.doc(), &d(v![p(v!["left base right"])]));
}

#[test]
fn maps_selection_when_rebasing() {
    let env = Env::new();
    let mut state = env.state(
        Some(d(v![p(v!["123456789ABCD"])])),
        HistoryOptions::default(),
        Vec::new(),
    );
    state = apply(&state, |tr| {
        let selection = TextSelection::create(&tr.doc, 6, 13).unwrap();
        tr.set_selection(selection);
    });
    state = apply(&state, |tr| {
        tr.delete(6, 13).unwrap();
    });
    state = apply(&state, |tr| {
        tr.insert(6, Fragment::from_node(text("6789ABC"))).unwrap();
        tr.insert(14, Fragment::from_node(text("E"))).unwrap();
        tr.delete(6, 13).unwrap();
        tr.set_meta(REBASED, 1usize);
        tr.set_meta(meta::ADD_TO_HISTORY, false);
        tr.mapping.set_mirror(0, 2);
    });
    let _ = command(&state, undo);
}

#[test]
fn a_composition_is_one_undo_step_and_typing_after_it_can_be_separated() {
    // Steps of one IME composition merge regardless of timing; closing
    // the history after the commit keeps later typing separate.
    let env = Env::new();
    let mut state = type_text(&env.default_state(), "Halo ");
    state = close(&state);
    for (i, partial) in ["あ", "あい"].iter().enumerate() {
        env.clock.advance(2_000);
        state = apply(&state, |tr| {
            let start = 6;
            let end = start + i;
            tr.insert_text_at(partial, start, end).unwrap();
            tr.set_meta(meta::COMPOSITION, 7u64);
        });
    }
    assert_eq!(state.doc(), &d(v![p(v!["Halo あい"])]));
    state = close(&state);
    state = type_text(&state, "!");
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["Halo あい"])]));
    state = command(&state, undo);
    assert_eq!(state.doc(), &d(v![p(v!["Halo "])]));
}
