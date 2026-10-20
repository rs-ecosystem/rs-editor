use crate::api::NativeEditor;

#[test]
fn the_kotlin_class_wraps_the_shared_api_in_pixels() {
    let mut editor = NativeEditor::new(1600.0, 1200.0, 2.0, false, "<p>halo</p>".into(), false, 0);
    assert_eq!(editor.get_html(), "<p>halo</p>");
    assert!(editor.run("selectAll".into(), "null".into()));
    assert!(editor.run_chain(r#"[{"name": "toggleBold"}]"#.into()));
    assert!(editor.is_active("bold".into(), "{}".into()));
    // ArrowLeft collapses the selection, so nothing is left to copy.
    assert!(editor.key_down("ArrowLeft".into(), false, false, false));
    assert!(!editor.copy(false).contains("halo"));
}
