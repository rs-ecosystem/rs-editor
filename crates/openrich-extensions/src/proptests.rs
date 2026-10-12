//! Random editing sessions: whatever users type, press or click, the
//! editor never panics, the document stays schema-valid, and undoing
//! everything restores the initial document.

use openrich_commands::{KeyEvent, Modifiers};
use openrich_state::ManualClock;
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::{Editor, EditorOptions};

#[derive(Debug, Clone)]
enum Action {
    Type(String),
    Key(&'static str, Modifiers),
    Select(usize, usize),
    Command(&'static str, Value),
}

fn modifiers(shift: bool, ctrl: bool) -> Modifiers {
    Modifiers {
        shift,
        ctrl,
        ..Default::default()
    }
}

fn arb_action() -> impl Strategy<Value = Action> {
    let text = prop_oneof![
        Just("a".to_owned()),
        Just(" ".to_owned()),
        Just("#".to_owned()),
        Just("*".to_owned()),
        Just("-".to_owned()),
        Just("`".to_owned()),
        Just("[".to_owned()),
        Just("]".to_owned()),
        Just(">".to_owned()),
        Just("1.".to_owned()),
        Just("🙂".to_owned()),
        Just("é".to_owned()),
        Just("שלום".to_owned()),
        Just("https://x.dev ".to_owned()),
    ];
    let key = prop_oneof![
        Just("Enter"),
        Just("Backspace"),
        Just("Delete"),
        Just("Tab"),
        Just("ArrowDown"),
        Just("z"),
        Just("b"),
        Just("a"),
    ];
    let command = prop_oneof![
        Just(("toggleBold", Value::Null)),
        Just(("toggleItalic", Value::Null)),
        Just(("toggleCode", Value::Null)),
        Just(("toggleHeading", json!({"level": 2}))),
        Just(("toggleBulletList", Value::Null)),
        Just(("toggleOrderedList", Value::Null)),
        Just(("toggleTaskList", Value::Null)),
        Just(("toggleBlockquote", Value::Null)),
        Just(("toggleCodeBlock", Value::Null)),
        Just(("setHorizontalRule", Value::Null)),
        Just(("setHardBreak", Value::Null)),
        Just(("setTextAlign", json!({"alignment": "center"}))),
        Just(("setLink", json!({"href": "https://openrich.dev"}))),
        Just(("unsetLink", Value::Null)),
        Just(("clearNodes", Value::Null)),
        Just(("unsetAllMarks", Value::Null)),
        Just(("selectAll", Value::Null)),
        Just(("deleteSelection", Value::Null)),
        Just(("liftListItem", Value::Null)),
        Just(("sinkListItem", Value::Null)),
        Just(("undo", Value::Null)),
        Just(("redo", Value::Null)),
    ];
    prop_oneof![
        3 => text.prop_map(Action::Type),
        2 => (key, any::<bool>(), any::<bool>()).prop_map(|(key, shift, ctrl)| Action::Key(key, modifiers(shift, ctrl))),
        1 => (0usize..80, 0usize..80).prop_map(|(a, b)| Action::Select(a, b)),
        2 => command.prop_map(|(name, args)| Action::Command(name, args)),
    ]
}

/// Cases per run; raise with `PROPTEST_CASES` for longer bug hunts.
fn cases(default: u32) -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn random_sessions_keep_the_document_valid(actions in prop::collection::vec(arb_action(), 1..40)) {
        let clock = ManualClock::new(10_000);
        let mut editor = Editor::new(EditorOptions {
            content: Some(json!({"type": "doc", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Halo dunia"}]},
                {"type": "bulletList", "content": [{"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "item"}]}]}]}
            ]})),
            clock: Some(clock.clone()),
            ..Default::default()
        }).unwrap();
        let initial = editor.get_json();
        for action in &actions {
            clock.advance(1_000);
            match action {
                Action::Type(text) => {
                    editor.type_text(text);
                }
                Action::Key(key, modifiers) => {
                    editor.handle_key(&KeyEvent::new(key, *modifiers), None);
                }
                Action::Select(a, b) => {
                    editor.select(*a, *b);
                }
                Action::Command(name, args) => {
                    let _ = editor.run(name, args);
                }
            }
            prop_assert!(editor.doc().check().is_ok(), "invalid after {action:?}: {}", editor.doc());
            let json = editor.get_json();
            prop_assert_eq!(editor.schema().node_from_json(&json).unwrap(), editor.doc().clone());
        }
        // Undo is bounded by the history depth, so only check when the
        // session fits in it.
        if actions.len() < 90 {
            while editor.run("undo", &Value::Null).unwrap() {}
            prop_assert_eq!(editor.get_json(), initial);
        }
    }
}
