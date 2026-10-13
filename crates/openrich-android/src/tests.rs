use serde_json::{Value, json};

use crate::api::NativeEditor;

fn editor(content: Value) -> NativeEditor {
    NativeEditor::new(800.0, 600.0, 2.0, false, content.to_string())
}

fn sample() -> NativeEditor {
    editor(json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": "satu dua"}]},
    ]}))
}

/// The commands `OpenRichChain` (Kotlin) offers, with example arguments.
const KOTLIN_CHAIN: &[(&str, &str)] = &[
    ("toggleBold", "null"),
    ("toggleItalic", "null"),
    ("toggleUnderline", "null"),
    ("toggleStrike", "null"),
    ("toggleCode", "null"),
    ("toggleHighlight", "null"),
    ("toggleSubscript", "null"),
    ("toggleSuperscript", "null"),
    ("unsetAllMarks", "null"),
    ("setParagraph", "null"),
    ("toggleHeading", r#"{"level": 2}"#),
    ("toggleBulletList", "null"),
    ("toggleOrderedList", "null"),
    ("toggleTaskList", "null"),
    ("toggleBlockquote", "null"),
    ("toggleCodeBlock", "null"),
    ("setHorizontalRule", "null"),
    ("setHardBreak", "null"),
    ("sinkListItem", "null"),
    ("liftListItem", "null"),
    ("setLink", r#"{"href": "https://openrich.dev"}"#),
    ("unsetLink", "null"),
    ("setTextAlign", r#""center""#),
    ("unsetTextAlign", "null"),
    ("selectAll", "null"),
    ("undo", "null"),
    ("redo", "null"),
];

#[test]
fn every_kotlin_chain_command_exists() {
    let mut editor = sample();
    for (name, args) in KOTLIN_CHAIN {
        let steps = json!([{"name": name, "args": serde_json::from_str::<Value>(args).unwrap()}]);
        // A dry run of an unknown command is an error, which reads as false
        // like an inapplicable one, so ask the extension manager directly.
        let args = serde_json::from_str(args).unwrap();
        assert!(
            editor.view.editor().manager().command(name, &args).is_ok(),
            "{name} is not a command"
        );
        let _ = editor.can_chain(steps.to_string());
    }
}

#[test]
fn chains_run_as_one_step_and_report_active_state() {
    let mut editor = sample();
    assert!(editor.run("selectAll".into(), "null".into()));
    let steps = r#"[{"name": "toggleBold"}, {"name": "toggleHeading", "args": {"level": 2}}]"#;
    assert!(editor.can_chain(steps.into()));
    let revision = editor.revision();
    assert!(editor.run_chain(steps.into()));
    assert!(editor.revision() > revision);
    assert!(editor.is_active("bold".into(), "{}".into()));
    assert!(editor.is_active("heading".into(), r#"{"level": 2}"#.into()));
    assert!(!editor.is_active("heading".into(), r#"{"level": 3}"#.into()));
    assert_eq!(
        serde_json::from_str::<Value>(&editor.get_attributes("heading".into())).unwrap()["level"],
        json!(2)
    );
    assert!(editor.run("undo".into(), "".into()));
    assert!(!editor.is_active("bold".into(), "".into()));
    assert!(!editor.run_chain("not json".into()));
    assert!(!editor.run_chain(r#"[{"name": "noSuchCommand"}]"#.into()));
}

#[test]
fn content_round_trips_through_json_and_html() {
    let mut doc = sample();
    assert_eq!(doc.get_text(), "satu dua");
    assert!(doc.set_html("<h2>Judul</h2><ul><li><p><strong>a</strong></p></li></ul>".into()));
    assert_eq!(
        doc.get_html(),
        "<h2>Judul</h2><ul><li><p><strong>a</strong></p></li></ul>"
    );
    let json = doc.get_json();
    let mut copy = editor(serde_json::from_str(&json).unwrap());
    assert_eq!(copy.get_json(), json);
    assert!(copy.set_html(String::new()));
    assert!(copy.is_empty());
    assert!(!copy.set_json("[1, 2]".into()));
    assert!(copy.set_json(json.clone()));
    assert_eq!(copy.get_json(), json);
}

#[test]
fn initial_content_can_be_html_and_is_not_undoable() {
    let mut editor = NativeEditor::new(
        800.0,
        600.0,
        2.0,
        false,
        "<p>halo <em>dunia</em></p>".into(),
    );
    assert_eq!(editor.get_html(), "<p>halo <em>dunia</em></p>");
    assert!(!editor.run("undo".into(), "".into()));
}

#[test]
fn bad_content_falls_back_to_an_empty_document() {
    let editor = NativeEditor::new(800.0, 600.0, 2.0, true, r#"{"type": "nope"}"#.into());
    assert!(editor.is_empty());
}

#[test]
fn read_only_editors_ignore_typing() {
    let mut editor = sample();
    editor.set_editable(false);
    assert!(!editor.is_editable());
    editor.commit_text("x".into());
    assert_eq!(editor.get_text(), "satu dua");
}
