//! End-to-end behavior of the StarterKit editor.

use std::sync::{Arc, Mutex};

use rs_editor_commands::{KeyEvent, KeyPlatform, Modifiers};
use rs_editor_state::ManualClock;
use serde_json::{Map, Value, json};

use crate::starter_kit::*;
use crate::*;

fn editor_with(content: Value) -> Editor {
    let clock = ManualClock::new(1_000);
    Editor::new(EditorOptions {
        content: Some(content),
        clock: Some(clock),
        ..Default::default()
    })
    .unwrap()
}

fn empty_editor() -> Editor {
    editor_with(json!({"type": "doc", "content": [{"type": "paragraph"}]}))
}

fn p(text: &str) -> Value {
    if text.is_empty() {
        json!({"type": "paragraph"})
    } else {
        json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
    }
}

fn doc(blocks: Vec<Value>) -> Value {
    json!({"type": "doc", "content": blocks})
}

/// The document JSON without default attributes, for compact assertions.
fn content(editor: &Editor) -> Value {
    editor.get_json()["content"].clone()
}

fn type_chars(editor: &mut Editor, text: &str) {
    for ch in text.chars() {
        assert!(editor.type_text(&ch.to_string()), "typing {ch:?}");
    }
}

fn press(editor: &mut Editor, key: &str, modifiers: Modifiers) -> bool {
    editor.handle_key(&KeyEvent::new(key, modifiers), None)
}

fn plain() -> Modifiers {
    Modifiers::default()
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Default::default()
    }
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Default::default()
    }
}

fn no_attrs() -> Map<String, Value> {
    Map::new()
}

// ---- schema and content ------------------------------------------------------

#[test]
fn starter_kit_schema_uses_tiptap_names_and_paragraph_default() {
    let editor = empty_editor();
    let schema = editor.schema();
    for name in [
        "doc",
        "paragraph",
        "text",
        "heading",
        "blockquote",
        "codeBlock",
        "horizontalRule",
        "hardBreak",
        "bulletList",
        "orderedList",
        "listItem",
        "taskList",
        "taskItem",
        "image",
    ] {
        assert!(schema.node_type(name).is_ok(), "missing node {name}");
    }
    for name in [
        "bold",
        "italic",
        "underline",
        "strike",
        "code",
        "link",
        "highlight",
        "subscript",
        "superscript",
    ] {
        assert!(schema.mark_type(name).is_ok(), "missing mark {name}");
    }
    let default = schema
        .top_node_type()
        .content_match()
        .default_type()
        .unwrap();
    assert_eq!(default.name(), "paragraph");
    assert!(editor.is_empty());
}

#[test]
fn tiptap_json_round_trips() {
    let source = json!({"type": "doc", "content": [
        {"type": "heading", "attrs": {"level": 2, "textAlign": "center"}, "content": [{"type": "text", "text": "Title"}]},
        {"type": "paragraph", "attrs": {"textAlign": "left"}, "content": [
            {"type": "text", "text": "Hi "},
            {"type": "text", "marks": [{"type": "link", "attrs": {"href": "https://x.dev", "target": null, "rel": "noopener noreferrer nofollow", "class": null}}, {"type": "bold"}], "text": "there"}
        ]},
        {"type": "taskList", "content": [
            {"type": "taskItem", "attrs": {"checked": true}, "content": [{"type": "paragraph", "attrs": {"textAlign": "left"}, "content": [{"type": "text", "text": "done"}]}]}
        ]}
    ]});
    let editor = editor_with(source.clone());
    assert_eq!(editor.get_json(), source);
    assert_eq!(editor.get_text("\n"), "Title\nHi there\ndone");
}

#[test]
fn invalid_content_is_rejected() {
    let result = Editor::new(EditorOptions {
        content: Some(json!({"type": "doc", "content": [{"type": "table"}]})),
        ..Default::default()
    });
    assert!(matches!(result, Err(ExtensionError::Schema(_))));
}

// ---- input rules --------------------------------------------------------------

#[test]
fn markdown_block_shortcuts() {
    let cases: Vec<(&str, Value)> = vec![
        (
            "# ",
            json!({"type": "heading", "attrs": {"level": 1, "textAlign": "left"}}),
        ),
        (
            "### ",
            json!({"type": "heading", "attrs": {"level": 3, "textAlign": "left"}}),
        ),
        (
            "> ",
            json!({"type": "blockquote", "content": [{"type": "paragraph", "attrs": {"textAlign": "left"}}]}),
        ),
        (
            "- ",
            json!({"type": "bulletList", "content": [{"type": "listItem", "content": [{"type": "paragraph", "attrs": {"textAlign": "left"}}]}]}),
        ),
        (
            "3. ",
            json!({"type": "orderedList", "attrs": {"start": 3, "type": null}, "content": [{"type": "listItem", "content": [{"type": "paragraph", "attrs": {"textAlign": "left"}}]}]}),
        ),
        (
            "[x] ",
            json!({"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": true}, "content": [{"type": "paragraph", "attrs": {"textAlign": "left"}}]}]}),
        ),
        (
            "```rust ",
            json!({"type": "codeBlock", "attrs": {"language": "rust"}}),
        ),
    ];
    for (typed, expected) in cases {
        let mut editor = empty_editor();
        type_chars(&mut editor, typed);
        assert_eq!(content(&editor)[0], expected, "typing {typed:?}");
    }
}

#[test]
fn horizontal_rule_shortcut() {
    let mut editor = empty_editor();
    type_chars(&mut editor, "---");
    assert_eq!(content(&editor)[0]["type"], "horizontalRule");
    assert_eq!(content(&editor)[1]["type"], "paragraph");
}

#[test]
fn markdown_mark_shortcuts() {
    let cases = [
        ("**bold**", "bold", "bold"),
        ("__bold__", "bold", "bold"),
        ("*it*", "italic", "it"),
        ("_it_", "italic", "it"),
        ("~~gone~~", "strike", "gone"),
        ("`code`", "code", "code"),
        ("==mark==", "highlight", "mark"),
    ];
    for (typed, mark, text) in cases {
        let mut editor = empty_editor();
        type_chars(&mut editor, &format!("say {typed}"));
        let inline = &content(&editor)[0]["content"];
        assert_eq!(inline[0]["text"], "say ", "{typed}");
        assert_eq!(inline[1]["text"], text, "{typed}");
        assert_eq!(inline[1]["marks"][0]["type"], mark, "{typed}");
        // Typing continues without the mark.
        assert!(editor.type_text("!"));
        let inline = &content(&editor)[0]["content"];
        assert_eq!(inline[2]["text"], "!", "{typed}");
        assert!(inline[2].get("marks").is_none(), "{typed}");
    }
}

#[test]
fn mark_shortcuts_ignore_whitespace_padding_and_code() {
    let mut editor = empty_editor();
    type_chars(&mut editor, "a ** b**");
    assert_eq!(content(&editor)[0]["content"][0]["text"], "a ** b**");
    let mut editor = empty_editor();
    type_chars(&mut editor, "```js ");
    type_chars(&mut editor, "**x**");
    assert_eq!(
        content(&editor)[0]["content"][0]["text"],
        "**x**",
        "no rules in code blocks"
    );
}

#[test]
fn backspace_undoes_an_input_rule() {
    let mut editor = empty_editor();
    type_chars(&mut editor, "# ");
    assert_eq!(content(&editor)[0]["type"], "heading");
    assert!(press(&mut editor, "Backspace", plain()));
    assert_eq!(
        content(&editor)[0],
        json!({"type": "paragraph", "attrs": {"textAlign": "left"}, "content": [{"type": "text", "text": "# "}]})
    );
}

#[test]
fn typography_rules() {
    let mut editor = empty_editor();
    type_chars(&mut editor, "wait... \"yes\" -- it's 1/2 (c)");
    assert_eq!(
        content(&editor)[0]["content"][0]["text"],
        "wait… “yes” — it’s ½ ©"
    );
}

// ---- commands and active state ------------------------------------------------

#[test]
fn toggle_bold_on_a_selection_and_report_active_state() {
    let mut editor = editor_with(doc(vec![p("hello world")]));
    editor.select(1, 6);
    assert!(!editor.is_active("bold", &no_attrs()));
    assert!(editor.run("toggleBold", &Value::Null).unwrap());
    assert!(editor.is_active("bold", &no_attrs()));
    assert_eq!(
        content(&editor)[0]["content"][0]["marks"][0]["type"],
        "bold"
    );
    assert!(editor.run("toggleBold", &Value::Null).unwrap());
    assert!(!editor.is_active("bold", &no_attrs()));
    assert!(content(&editor)[0]["content"][0].get("marks").is_none());
}

#[test]
fn stored_marks_apply_to_typed_text() {
    let mut editor = empty_editor();
    assert!(press(&mut editor, "b", ctrl()));
    assert!(editor.is_active("bold", &no_attrs()));
    type_chars(&mut editor, "hi");
    assert_eq!(
        content(&editor)[0]["content"][0]["marks"][0]["type"],
        "bold"
    );
}

#[test]
fn headings_toggle_and_validate_levels() {
    let mut editor = editor_with(doc(vec![p("Title")]));
    editor.select(2, 2);
    assert!(editor.run("toggleHeading", &json!({"level": 2})).unwrap());
    assert!(editor.is_active(
        "heading",
        &json!({"level": 2}).as_object().cloned().unwrap()
    ));
    assert!(!editor.is_active(
        "heading",
        &json!({"level": 1}).as_object().cloned().unwrap()
    ));
    assert!(editor.run("toggleHeading", &json!({"level": 2})).unwrap());
    assert_eq!(content(&editor)[0]["type"], "paragraph");
    assert!(matches!(
        editor.run("setHeading", &json!({"level": 9})),
        Err(ExtensionError::InvalidArguments { .. })
    ));
}

#[test]
fn lists_toggle_switch_and_unwrap() {
    let mut editor = editor_with(doc(vec![p("one"), p("two")]));
    editor.select(2, 7);
    assert!(editor.run("toggleBulletList", &Value::Null).unwrap());
    assert_eq!(content(&editor)[0]["type"], "bulletList");
    assert_eq!(content(&editor)[0]["content"].as_array().unwrap().len(), 2);
    assert!(editor.is_active("bulletList", &no_attrs()));
    assert!(editor.run("toggleOrderedList", &Value::Null).unwrap());
    assert_eq!(content(&editor)[0]["type"], "orderedList");
    assert!(editor.run("toggleOrderedList", &Value::Null).unwrap());
    assert_eq!(content(&editor)[0]["type"], "paragraph");
    assert_eq!(content(&editor)[1]["type"], "paragraph");
}

#[test]
fn set_link_validates_and_unset_link_removes_the_whole_link() {
    let mut editor = editor_with(doc(vec![p("visit site now")]));
    editor.select(7, 11);
    assert!(matches!(
        editor.run("setLink", &json!({"href": "javascript:alert(1)"})),
        Err(ExtensionError::InvalidArguments { .. })
    ));
    assert!(
        editor
            .run("setLink", &json!({"href": "https://rs-editor.dev"}))
            .unwrap()
    );
    assert_eq!(
        editor.get_attributes("link")["href"],
        "https://rs-editor.dev"
    );
    editor.select(9, 9);
    assert!(editor.is_active("link", &no_attrs()));
    assert!(editor.run("unsetLink", &Value::Null).unwrap());
    assert!(content(&editor)[0]["content"][0].get("marks").is_none());
}

#[test]
fn autolink_marks_urls_after_a_space() {
    let mut editor = empty_editor();
    type_chars(&mut editor, "see https://rs-editor.dev, ");
    let inline = &content(&editor)[0]["content"];
    assert_eq!(inline[1]["text"], "https://rs-editor.dev");
    assert_eq!(
        inline[1]["marks"][0]["attrs"]["href"],
        "https://rs-editor.dev"
    );
    assert_eq!(
        detect_link("www.rust-lang.org"),
        Some("https://www.rust-lang.org".into())
    );
    assert_eq!(
        detect_link("me@example.com"),
        Some("mailto:me@example.com".into())
    );
    assert_eq!(detect_link("hello"), None);
}

#[test]
fn text_align_updates_blocks() {
    let mut editor = editor_with(doc(vec![p("a"), p("b")]));
    editor.select(1, 5);
    assert!(
        editor
            .run("setTextAlign", &json!({"alignment": "center"}))
            .unwrap()
    );
    assert_eq!(content(&editor)[0]["attrs"]["textAlign"], "center");
    assert_eq!(content(&editor)[1]["attrs"]["textAlign"], "center");
    assert!(
        editor
            .run("setTextAlign", &json!({"alignment": "sideways"}))
            .is_err()
    );
}

#[test]
fn insert_content_accepts_text_and_nodes() {
    let mut editor = editor_with(doc(vec![p("ab")]));
    editor.select(2, 2);
    assert!(editor.run("insertContent", &json!("XY")).unwrap());
    assert_eq!(content(&editor)[0]["content"][0]["text"], "aXYb");
    assert!(
        editor
            .run("insertContent", &json!({"type": "horizontalRule"}))
            .unwrap()
    );
    assert_eq!(content(&editor)[1]["type"], "horizontalRule");
}

#[test]
fn can_reports_applicability_without_changing_anything() {
    let mut editor = editor_with(doc(vec![
        json!({"type": "codeBlock", "content": [{"type": "text", "text": "x"}]}),
    ]));
    editor.select(1, 2);
    let before = editor.get_json();
    assert!(
        !editor.can("toggleBold", &Value::Null),
        "code blocks allow no marks"
    );
    assert!(editor.can("setParagraph", &Value::Null));
    assert_eq!(editor.get_json(), before);
}

#[test]
fn can_judges_compound_commands_as_they_would_run() {
    // Bullet list to task list clears the list, then wraps again: the
    // second step only applies after the first.
    let mut editor = editor_with(doc(vec![json!({"type": "bulletList", "content": [
        {"type": "listItem", "content": [p("satu")]},
    ]})]));
    editor.select(3, 3);
    let before = editor.get_json();
    assert!(editor.can("toggleTaskList", &Value::Null));
    assert_eq!(editor.get_json(), before);
    assert!(editor.run("toggleTaskList", &Value::Null).unwrap());
    assert_eq!(content(&editor)[0]["type"], "taskList");
}

#[test]
fn a_chain_is_a_single_undo_step() {
    let mut editor = editor_with(doc(vec![p("text")]));
    editor.select(1, 5);
    assert!(
        editor
            .chain()
            .cmd("toggleBold", Value::Null)
            .cmd("toggleItalic", Value::Null)
            .cmd("toggleHeading", json!({"level": 1}))
            .run()
            .unwrap()
    );
    assert_eq!(content(&editor)[0]["type"], "heading");
    assert_eq!(
        content(&editor)[0]["content"][0]["marks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(editor.run("undo", &Value::Null).unwrap());
    assert_eq!(
        content(&editor)[0],
        json!({"type": "paragraph", "attrs": {"textAlign": "left"}, "content": [{"type": "text", "text": "text"}]})
    );
}

// ---- keyboard ------------------------------------------------------------------

#[test]
fn enter_tab_and_backspace_in_lists() {
    let mut editor = empty_editor();
    type_chars(&mut editor, "- one");
    assert!(press(&mut editor, "Enter", plain()));
    type_chars(&mut editor, "two");
    assert!(press(&mut editor, "Tab", plain()));
    let list = &content(&editor)[0];
    assert_eq!(
        list["content"].as_array().unwrap().len(),
        1,
        "two sank into one"
    );
    assert_eq!(list["content"][0]["content"][1]["type"], "bulletList");
    assert!(press(&mut editor, "Tab", shift()));
    assert_eq!(content(&editor)[0]["content"].as_array().unwrap().len(), 2);
    // Enter on an empty item leaves the list.
    assert!(press(&mut editor, "Enter", plain()));
    assert!(press(&mut editor, "Enter", plain()));
    assert_eq!(content(&editor)[1]["type"], "paragraph");
}

#[test]
fn backspace_turns_an_empty_first_heading_into_a_paragraph() {
    let mut editor = empty_editor();
    assert!(editor.run("setHeading", &json!({"level": 1})).unwrap());
    assert!(press(&mut editor, "Backspace", plain()));
    assert_eq!(content(&editor)[0]["type"], "paragraph");
}

#[test]
fn triple_enter_leaves_a_code_block() {
    let mut editor = empty_editor();
    type_chars(&mut editor, "```js ");
    type_chars(&mut editor, "x");
    for _ in 0..3 {
        assert!(press(&mut editor, "Enter", plain()));
    }
    assert_eq!(
        content(&editor)[0],
        json!({"type": "codeBlock", "attrs": {"language": "js"}, "content": [{"type": "text", "text": "x"}]})
    );
    assert_eq!(content(&editor)[1]["type"], "paragraph");
}

#[test]
fn shift_enter_inserts_a_hard_break_and_mod_z_undoes() {
    let mut editor = empty_editor();
    type_chars(&mut editor, "a");
    assert!(press(&mut editor, "Enter", shift()));
    type_chars(&mut editor, "b");
    assert_eq!(content(&editor)[0]["content"][1]["type"], "hardBreak");
    assert!(press(&mut editor, "z", ctrl()));
    assert!(editor.is_empty(), "typing burst undone as one step");
}

#[test]
fn apple_platform_uses_command_for_mod() {
    let mut editor = Editor::new(EditorOptions {
        content: Some(doc(vec![p("hi")])),
        platform: KeyPlatform::Apple,
        ..Default::default()
    })
    .unwrap();
    editor.select(1, 3);
    let cmd = Modifiers {
        meta: true,
        ..Default::default()
    };
    assert!(press(&mut editor, "b", cmd));
    assert!(editor.is_active("bold", &no_attrs()));
    assert!(!press(&mut editor, "q", ctrl()));
}

#[test]
fn read_only_editors_ignore_input() {
    let mut editor = editor_with(doc(vec![p("hi")]));
    editor.set_editable(false);
    assert!(!editor.type_text("x"));
    assert!(!press(&mut editor, "Enter", plain()));
    assert_eq!(content(&editor)[0]["content"][0]["text"], "hi");
}

// ---- utilities -----------------------------------------------------------------

#[test]
fn character_count_counts_graphemes_and_enforces_limits() {
    let editor = editor_with(doc(vec![p("hi 👩‍💻"), p("there")]));
    let counter = CharacterCount::default();
    assert_eq!(counter.characters(editor.doc()), 9);
    assert_eq!(counter.words(editor.doc()), 2);

    let mut extensions = starter_kit();
    extensions.retain(|e| e.name() != "characterCount");
    extensions.push(Arc::new(CharacterCount {
        limit: Some(3),
        mode: CountMode::Graphemes,
    }));
    let mut limited = Editor::new(EditorOptions {
        extensions,
        ..Default::default()
    })
    .unwrap();
    type_chars(&mut limited, "abc");
    assert!(!limited.type_text("d") || limited.get_text("\n") == "abc");
    assert_eq!(limited.get_text("\n"), "abc");
}

#[test]
fn placeholder_text_per_node_type() {
    let editor = editor_with(doc(vec![json!({"type": "heading", "attrs": {"level": 1}})]));
    let placeholder = Placeholder {
        per_type: vec![("heading".into(), "Title".into())],
        ..Default::default()
    };
    assert_eq!(
        placeholder.text_for(editor.doc().child(0), true),
        Some("Title")
    );
    let filled = editor_with(doc(vec![p("x")]));
    assert_eq!(placeholder.text_for(filled.doc().child(0), false), None);
}

#[test]
fn listeners_receive_update_and_selection_events() {
    let mut editor = editor_with(doc(vec![p("hi")]));
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    editor.on(Box::new(move |event, _state| {
        sink.lock().unwrap().push(event)
    }));
    editor.select(1, 2);
    editor.type_text("x");
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            EditorEvent::Transaction,
            EditorEvent::SelectionUpdate,
            EditorEvent::Transaction,
            EditorEvent::Update
        ]
    );
}

#[test]
fn task_items_toggle_through_the_command() {
    let mut editor = empty_editor();
    type_chars(&mut editor, "[ ] buy milk");
    let pos = 1;
    assert!(editor.exec(&toggle_task_item(pos), None));
    assert_eq!(content(&editor)[0]["content"][0]["attrs"]["checked"], true);
}

#[test]
fn autolink_does_not_relink_after_undo_or_block_changes() {
    let mut editor = empty_editor();
    type_chars(&mut editor, "go https://x.dev ");
    assert_eq!(
        content(&editor)[0]["content"][1]["marks"][0]["type"],
        "link"
    );
    editor.select(1, 19);
    assert!(editor.run("unsetAllMarks", &Value::Null).unwrap());
    let unlinked = editor.get_json();
    editor.select(18, 18);
    assert!(editor.run("toggleHeading", &json!({"level": 1})).unwrap());
    assert!(
        content(&editor)[0]["content"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n.get("marks").is_none()),
        "changing the block type must not autolink"
    );
    assert!(editor.run("undo", &Value::Null).unwrap());
    assert_eq!(editor.get_json(), unlinked, "undo must not autolink either");
}

#[test]
fn detected_links_keep_balanced_parentheses() {
    use crate::starter_kit::detect_link;
    assert_eq!(
        detect_link("https://en.wikipedia.org/wiki/Rust_(programming_language)").as_deref(),
        Some("https://en.wikipedia.org/wiki/Rust_(programming_language)")
    );
    assert_eq!(
        detect_link("https://a.b/x),").as_deref(),
        Some("https://a.b/x"),
        "a sentence's parenthesis and comma"
    );
    assert_eq!(detect_link("www.a.b.").as_deref(), Some("https://www.a.b"));
}
