use rs_editor_extensions::{Editor, EditorOptions};
use rs_editor_model::{Fragment, Schema, Slice, fragment_to_json};
use serde_json::{Value, json};

use crate::*;

fn schema() -> Schema {
    Editor::new(EditorOptions::default())
        .expect("editor")
        .schema()
        .clone()
}

fn json_of(fragment: &Fragment) -> Value {
    fragment_to_json(fragment)
}

fn text(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

fn marked(text: &str, marks: Value) -> Value {
    json!({"type": "text", "text": text, "marks": marks})
}

fn p(content: Vec<Value>) -> Value {
    json!({"type": "paragraph", "content": content})
}

fn parse(html: &str) -> Value {
    json_of(&parse_fragment(&schema(), html))
}

/// `content` as the schema stores it (with default attributes).
fn expected(content: Value) -> Value {
    json_of(
        &schema()
            .fragment_from_json(&content)
            .expect("valid content"),
    )
}

macro_rules! expect {
    ($($json:tt)+) => {
        expected(json!($($json)+))
    };
}

#[test]
fn every_starter_kit_node_and_mark_round_trips() {
    let schema = schema();
    let doc = json!([
        {"type": "heading", "attrs": {"level": 2, "textAlign": "center"}, "content": [text("Judul")]},
        p(vec![
            text("a "),
            marked("tebal", json!([{"type": "bold"}])),
            text(" "),
            marked("miring tebal", json!([{"type": "bold"}, {"type": "italic"}])),
            text(", "),
            marked("u", json!([{"type": "underline"}])),
            marked("s", json!([{"type": "strike"}])),
            marked("kode", json!([{"type": "code"}])),
            marked("2", json!([{"type": "subscript"}])),
            marked("3", json!([{"type": "superscript"}])),
            marked("sorot", json!([{"type": "highlight"}])),
            text(" "),
            marked("tautan", json!([{"type": "link", "attrs": {"href": "https://contoh.id", "target": "_blank", "rel": "noopener noreferrer nofollow", "class": null}}])),
            json!({"type": "hardBreak"}),
            text("baris 2 < & >"),
        ]),
        {"type": "blockquote", "content": [p(vec![text("kutipan")])]},
        {"type": "bulletList", "content": [
            {"type": "listItem", "content": [
                p(vec![text("satu")]),
                {"type": "bulletList", "content": [{"type": "listItem", "content": [p(vec![text("dalam")])]}]},
            ]},
        ]},
        {"type": "orderedList", "attrs": {"start": 3, "type": null}, "content": [{"type": "listItem", "content": [p(vec![text("tiga")])]}]},
        {"type": "taskList", "content": [
            {"type": "taskItem", "attrs": {"checked": true}, "content": [p(vec![text("selesai")])]},
            {"type": "taskItem", "attrs": {"checked": false}, "content": [p(vec![text("belum")])]},
        ]},
        {"type": "codeBlock", "attrs": {"language": "rust"}, "content": [text("fn main() {\n    a  <  b;\n}")]},
        {"type": "horizontalRule"},
    ]);
    let fragment = schema.fragment_from_json(&doc).expect("valid document");
    let html = serialize_fragment(&fragment);
    let back = parse_fragment(&schema, &html);
    assert_eq!(json_of(&back), json_of(&fragment), "{html}");
}

#[test]
fn google_docs_html_keeps_real_formatting_only() {
    let html = r#"<meta charset="utf-8"><b style="font-weight:normal;" id="docs-internal-guid-1"><p dir="ltr" style="line-height:1.38;margin-top:0pt;"><span style="font-size:11pt;font-weight:700;">Tebal</span><span style="font-size:11pt;"> dan </span><span style="font-style:italic;text-decoration:underline;">miring</span></p><ul style="margin-top:0;"><li dir="ltr"><p dir="ltr"><span>satu</span></p></li></ul></b><br class="Apple-interchange-newline">"#;
    assert_eq!(
        parse(html),
        expect!([
            p(vec![
                marked("Tebal", json!([{"type": "bold"}])),
                text(" dan "),
                marked("miring", json!([{"type": "italic"}, {"type": "underline"}])),
            ]),
            {"type": "bulletList", "content": [{"type": "listItem", "content": [p(vec![text("satu")])]}]},
        ])
    );
}

#[test]
fn github_task_lists_become_task_items() {
    let html = r#"<ul class="contains-task-list"><li class="task-list-item"><input type="checkbox" checked disabled> selesai</li><li class="task-list-item"><input type="checkbox" disabled> belum</li></ul>"#;
    assert_eq!(
        parse(html),
        expect!([{"type": "taskList", "content": [
            {"type": "taskItem", "attrs": {"checked": true}, "content": [p(vec![text("selesai")])]},
            {"type": "taskItem", "attrs": {"checked": false}, "content": [p(vec![text("belum")])]},
        ]}])
    );
}

#[test]
fn loose_and_badly_nested_html_still_fits_the_schema() {
    assert_eq!(
        parse("halo <b>dunia</b><div>dua</div>tiga"),
        expect!([
            p(vec![
                text("halo "),
                marked("dunia", json!([{"type": "bold"}]))
            ]),
            p(vec![text("dua")]),
            p(vec![text("tiga")]),
        ])
    );
    assert_eq!(
        parse("<ul><li>satu<ul><li>dua</ul><li>tiga</ul>"),
        expect!([{"type": "bulletList", "content": [
            {"type": "listItem", "content": [
                p(vec![text("satu")]),
                {"type": "bulletList", "content": [{"type": "listItem", "content": [p(vec![text("dua")])]}]},
            ]},
            {"type": "listItem", "content": [p(vec![text("tiga")])]},
        ]}])
    );
    assert_eq!(
        parse("<li>tanpa list</li>"),
        expect!([{"type": "bulletList", "content": [{"type": "listItem", "content": [p(vec![text("tanpa list")])]}]}])
    );
    assert_eq!(
        parse("<table><tr><td>a</td><td>b</td></tr></table>"),
        expect!([p(vec![text("a")]), p(vec![text("b")])])
    );
}

#[test]
fn code_blocks_keep_whitespace_and_language() {
    assert_eq!(
        parse("<pre class=\"language-js\"><code>let  a = 1;<br>  b<b>!</b></code></pre>"),
        expect!([{"type": "codeBlock", "attrs": {"language": "js"}, "content": [text("let  a = 1;\n  b!")]}])
    );
}

#[test]
fn whitespace_collapses_like_a_browser() {
    assert_eq!(
        parse("<p>\n  satu   dua  </p>\n\n<p> tiga<b> empat</b> </p>"),
        expect!([
            p(vec![text("satu dua")]),
            p(vec![
                text("tiga"),
                marked(" empat", json!([{"type": "bold"}]))
            ])
        ])
    );
}

#[test]
fn unsafe_links_images_and_scripts_are_dropped() {
    assert_eq!(
        parse(
            "<script>alert(1)</script><style>p{}</style><p><a href=\"javascript:alert(1)\">x</a><img src=\"javascript:1\"></p>"
        ),
        expect!([p(vec![text("x")])])
    );
}

#[test]
fn the_windows_clipboard_header_is_ignored() {
    let html = "Version:0.9\r\nStartHTML:0000000105\r\n<html><body><!--StartFragment--><p>isi</p><!--EndFragment--></body></html>";
    assert_eq!(parse(html), expect!([p(vec![text("isi")])]));
}

#[test]
fn pasted_inline_html_merges_into_the_current_paragraph() {
    let slice = parse_slice(&schema(), "halo <i>dunia</i>");
    assert_eq!((slice.open_start, slice.open_end), (1, 1));
}

#[test]
fn rs_editor_clipboard_html_restores_the_exact_slice() {
    let schema = schema();
    let content = schema
        .fragment_from_json(&json!([p(vec![text("  dua spasi  ")]), p(vec![text("b")])]))
        .unwrap();
    let slice = Slice::new(content, 1, 1);
    let html = serialize_slice(&slice);
    let back = parse_slice(&schema, &html);
    assert_eq!(json_of(&back.content), json_of(&slice.content));
    assert_eq!((back.open_start, back.open_end), (1, 1));
    // Other apps see plain markup, with spaces kept visible.
    assert!(
        html.contains("<p>&nbsp; dua spasi &nbsp;</p><p>b</p>"),
        "{html}"
    );
}

mod proptests {
    use proptest::prelude::*;

    use super::*;

    const PIECES: &[&str] = &[
        "<p>",
        "</p>",
        "<ul>",
        "</ul>",
        "<ol start=\"3\">",
        "</ol>",
        "<li>",
        "</li>",
        "<b>",
        "</b>",
        "<i>",
        "</i>",
        "<code>",
        "</code>",
        "<pre>",
        "</pre>",
        "<br>",
        "<h2>",
        "</h2>",
        "<blockquote>",
        "</blockquote>",
        "<div>",
        "</div>",
        "<table><tr><td>",
        "</td>",
        "</table>",
        "<img src=\"https://a.id/b.png\">",
        "<input type=\"checkbox\" checked>",
        "<hr>",
        "<a href=\"https://a.id\">",
        "</a>",
        "<span style=\"font-weight:700\">",
        "</span>",
        "<ul data-type=\"taskList\">",
        "<li data-type=\"taskItem\" data-checked=\"true\">",
        "kata",
        " ",
        "  ",
        "\n",
        "&amp;",
        "👩‍💻",
        "<script>x</script>",
    ];

    fn cases(default: u32) -> u32 {
        std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(cases(300)))]

        #[test]
        fn any_tag_soup_parses_to_valid_content_and_round_trips(
            pieces in proptest::collection::vec(0..PIECES.len(), 0..40)
        ) {
            let html: String = pieces.iter().map(|&i| PIECES[i]).collect();
            let schema = schema();
            let fragment = parse_fragment(&schema, &html);
            prop_assert!(
                fragment.child_count() == 0 || schema.top_node_type().valid_content(&fragment),
                "invalid content from {html:?}: {}", json_of(&fragment)
            );
            let again = parse_fragment(&schema, &serialize_fragment(&fragment));
            prop_assert_eq!(json_of(&again), json_of(&fragment), "html {:?}", html);
        }
    }
}
