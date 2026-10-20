use crate::api::NativeEditor;

#[test]
fn the_swift_class_wraps_the_shared_api_in_points() {
    let mut editor = NativeEditor::new(
        800.0,
        600.0,
        2.0,
        false,
        "<p>halo</p>".into(),
        false,
        0,
        false,
    );
    assert_eq!(editor.get_html(), "<p>halo</p>");
    assert!(editor.run("selectAll".into(), "null".into()));
    // Command-B (meta = 8) on the Apple keymap.
    assert!(editor.key_down("b".into(), 8));
    assert!(editor.is_active("bold".into(), "{}".into()));
    assert!(editor.key_down("ArrowRight".into(), 0));
    assert!(
        editor.selection().contains(r#""from":5"#),
        "at the end of the text"
    );

    // Marked text (an input method composing), then committed.
    editor.set_marked_text("にほ".into());
    let state: serde_json::Value = serde_json::from_str(&editor.ime_state()).unwrap();
    assert_eq!(state["composition"], serde_json::json!([4, 6]));
    assert!(editor.replace_text(4, 6, "日本".into(), false));
    assert_eq!(editor.get_text(), "halo日本");

    // characterIndex(for:): a point on the text gives an offset in it.
    let info: serde_json::Value = serde_json::from_str(&editor.ime_cursor_info(0, 1)).unwrap();
    let bounds = &info["bounds"][0];
    let (x, y) = (
        bounds[0].as_f64().unwrap() + 1.0,
        bounds[1].as_f64().unwrap() + 4.0,
    );
    assert_eq!(editor.ime_offset_at(x as f32, y as f32), 0);
    assert_eq!(
        editor.ime_offset_at(700.0, y as f32),
        6,
        "right of the text: its end"
    );
}

#[test]
fn a_committed_composition_is_one_undo_step() {
    let mut editor = NativeEditor::new(
        800.0,
        600.0,
        2.0,
        false,
        "<p>halo</p>".into(),
        false,
        0,
        false,
    );
    assert!(editor.run("setTextSelection".into(), r#"{"from": 5, "to": 5}"#.into()));
    editor.set_marked_text("にほ".into());
    assert!(editor.replace_text(4, 6, "にほん".into(), true));
    editor.commit_text("日本".into());
    assert_eq!(editor.get_text(), "halo日本");
    assert!(editor.run("undo".into(), "null".into()));
    assert_eq!(editor.get_text(), "halo", "the whole word goes");
}
