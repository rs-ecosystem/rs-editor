use serde_json::{Value, json};

use crate::{ApiOptions, EditorApi};

fn with_content(content: &str, dark: bool) -> EditorApi {
    EditorApi::new(ApiOptions {
        width: 1600.0,
        height: 1200.0,
        scale: 2.0,
        units: crate::Units::Device,
        dark,
        content: content.to_string(),
        ..Default::default()
    })
}

fn editor(content: Value) -> EditorApi {
    with_content(&content.to_string(), false)
}

fn sample() -> EditorApi {
    editor(json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": "satu dua"}]},
    ]}))
}

/// The commands the platform chain builders (`RsEditorChain` in Kotlin,
/// `chain()` on the web) offer, with example arguments.
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
    ("setLink", r#"{"href": "https://rs-editor.dev"}"#),
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
            editor
                .view()
                .editor()
                .manager()
                .command(name, &args)
                .is_ok(),
            "{name} is not a command"
        );
        let _ = editor.can_chain(&steps.to_string());
    }
}

#[test]
fn chains_run_as_one_step_and_report_active_state() {
    let mut editor = sample();
    assert!(editor.run("selectAll", "null"));
    let steps = r#"[{"name": "toggleBold"}, {"name": "toggleHeading", "args": {"level": 2}}]"#;
    assert!(editor.can_chain(steps));
    let revision = editor.revision();
    assert!(editor.run_chain(steps));
    assert!(editor.revision() > revision);
    assert!(editor.is_active("bold", "{}"));
    assert!(editor.is_active("heading", r#"{"level": 2}"#));
    assert!(!editor.is_active("heading", r#"{"level": 3}"#));
    assert_eq!(
        serde_json::from_str::<Value>(&editor.get_attributes("heading")).unwrap()["level"],
        json!(2)
    );
    assert!(editor.run("undo", ""));
    assert!(!editor.is_active("bold", ""));
    assert!(!editor.run_chain("not json"));
    assert!(!editor.run_chain(r#"[{"name": "noSuchCommand"}]"#));
}

#[test]
fn content_round_trips_through_json_and_html() {
    let mut doc = sample();
    assert_eq!(doc.get_text(), "satu dua");
    assert!(doc.set_html("<h2>Judul</h2><ul><li><p><strong>a</strong></p></li></ul>"));
    assert_eq!(
        doc.get_html(),
        "<h2>Judul</h2><ul><li><p><strong>a</strong></p></li></ul>"
    );
    let json = doc.get_json();
    let mut copy = editor(serde_json::from_str(&json).unwrap());
    assert_eq!(copy.get_json(), json);
    assert!(copy.set_html(""));
    assert!(copy.is_empty());
    assert!(!copy.set_json("[1, 2]"));
    assert!(copy.set_json(&json));
    assert_eq!(copy.get_json(), json);
}

#[test]
fn initial_content_can_be_html_and_is_not_undoable() {
    let mut editor = with_content("<p>halo <em>dunia</em></p>", false);
    assert_eq!(editor.get_html(), "<p>halo <em>dunia</em></p>");
    assert!(!editor.run("undo", ""));
}

#[test]
fn bad_content_falls_back_to_an_empty_document() {
    let editor = with_content(r#"{"type": "nope"}"#, true);
    assert!(editor.is_empty());
}

#[test]
fn read_only_editors_ignore_typing() {
    let mut editor = sample();
    editor.set_editable(false);
    assert!(!editor.is_editable());
    editor.commit_text("x");
    assert_eq!(editor.get_text(), "satu dua");
}

#[test]
fn coordinates_follow_the_shell_units() {
    let content = json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": "satu dua tiga empat"}]},
    ]})
    .to_string();
    let mut logical = EditorApi::new(ApiOptions {
        width: 800.0,
        height: 600.0,
        scale: 2.0,
        units: crate::Units::Logical,
        content: content.clone(),
        ..Default::default()
    });
    let mut device = with_content(&content, false);
    let state = |api: &mut EditorApi| serde_json::from_str::<Value>(&api.ime_state()).unwrap();
    // The same tap, in each unit, puts the caret at the same place.
    for (api, scale) in [(&mut logical, 1.0), (&mut device, 2.0)] {
        api.set_focused(true);
        api.touch(0, 120.0 * scale, 40.0 * scale);
        api.touch(2, 120.0 * scale, 40.0 * scale);
    }
    let (a, b) = (state(&mut logical), state(&mut device));
    assert_eq!(a["selection"], b["selection"]);
    let x = |s: &Value| s["caret"][0].as_f64().unwrap();
    assert!((x(&a) * 2.0 - x(&b)).abs() < 0.01, "{a} {b}");
}

#[test]
fn content_round_trips_through_markdown() {
    let mut editor = sample();
    assert!(editor.set_markdown("# Judul\n\n- [x] **tugas**\n"));
    assert_eq!(editor.get_markdown(), "# Judul\n\n- [x] **tugas**");
    assert!(editor.set_markdown(""));
    assert!(editor.is_empty());
}

#[test]
fn initial_content_can_be_markdown() {
    let mut editor = EditorApi::new(ApiOptions {
        content: "## Hai *dunia*".into(),
        markdown: true,
        ..Default::default()
    });
    assert_eq!(editor.get_html(), "<h2>Hai <em>dunia</em></h2>");
    assert!(!editor.run("undo", ""), "not an undoable change");
}
