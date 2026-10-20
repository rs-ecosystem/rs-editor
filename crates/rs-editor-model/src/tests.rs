//! Ports of ProseMirror's model tests plus Rs Editor specific cases.

use serde_json::json;

use crate::testing::*;
use crate::v;
use crate::{Fragment, MarkSet, ModelError, Node, NodeSpec, Schema, Slice};

// ---- resolve ---------------------------------------------------------------

#[test]
fn resolve_reflects_the_document_structure() {
    let test_doc = doc(v![p(v!["ab"]), blockquote(v![p(v![em(v!["cd"]), "ef"])])]).node;
    let p1 = test_doc.child(0).clone();
    let blk = test_doc.child(1).clone();
    let p2 = blk.child(0).clone();
    // (node, start, end) per depth, then parent offset, text before, text after.
    let d = (&test_doc, 0, 12);
    let a = (&p1, 1, 3);
    let b = (&blk, 5, 11);
    let c = (&p2, 6, 10);
    type Level<'a> = (&'a Node, usize, usize);
    type Expected<'a> = (Vec<Level<'a>>, usize, Option<&'a str>, Option<&'a str>);
    let expected: Vec<Expected> = vec![
        (vec![d], 0, None, Some("ab")),
        (vec![d, a], 0, None, Some("ab")),
        (vec![d, a], 1, Some("a"), Some("b")),
        (vec![d, a], 2, Some("ab"), None),
        (vec![d], 4, Some("ab"), Some("cdef")),
        (vec![d, b], 0, None, Some("cdef")),
        (vec![d, b, c], 0, None, Some("cd")),
        (vec![d, b, c], 1, Some("c"), Some("d")),
        (vec![d, b, c], 2, Some("cd"), Some("ef")),
        (vec![d, b, c], 3, Some("e"), Some("f")),
        (vec![d, b, c], 4, Some("ef"), None),
        (vec![d, b], 6, Some("cdef"), None),
        (vec![d], 12, Some("cdef"), None),
    ];
    for (pos, (levels, parent_offset, before, after)) in expected.into_iter().enumerate() {
        let resolved = test_doc.resolve(pos).unwrap();
        assert_eq!(resolved.depth(), levels.len() - 1, "depth at {pos}");
        for (depth, (node, start, end)) in levels.iter().enumerate() {
            assert_eq!(resolved.node(depth), *node, "node {depth} at {pos}");
            assert_eq!(resolved.start(depth), *start, "start {depth} at {pos}");
            assert_eq!(resolved.end(depth), *end, "end {depth} at {pos}");
            if depth > 0 {
                assert_eq!(resolved.before(depth), start - 1);
                assert_eq!(resolved.after(depth), end + 1);
            }
        }
        assert_eq!(
            resolved.parent_offset(),
            parent_offset,
            "parent offset at {pos}"
        );
        assert_eq!(
            resolved.node_before().map(|n| n.text_content()).as_deref(),
            before,
            "before {pos}"
        );
        assert_eq!(
            resolved.node_after().map(|n| n.text_content()).as_deref(),
            after,
            "after {pos}"
        );
    }
}

#[test]
fn resolve_rejects_positions_outside_or_inside_a_surrogate_pair() {
    let d = doc(v![p(v!["a🙂b"])]).node;
    assert_eq!(d.content().size(), 6);
    assert!(d.resolve(2).is_ok());
    assert_eq!(
        d.resolve(3).unwrap_err(),
        ModelError::PositionInsideCharacter(3)
    );
    assert!(d.resolve(4).is_ok());
    assert_eq!(
        d.resolve(7).unwrap_err(),
        ModelError::PositionOutOfRange { pos: 7, size: 6 }
    );
}

// ---- slice -----------------------------------------------------------------

fn check_slice(tagged: TaggedNode, expected: Fragment, open_start: usize, open_end: usize) {
    let from = tagged.tags.get("a").copied().unwrap_or(0);
    let to = tagged
        .tags
        .get("b")
        .copied()
        .unwrap_or(tagged.node.content().size());
    let slice = tagged.node.slice(from, to, false).unwrap();
    assert_eq!(slice.content, expected, "{} vs {}", slice.content, expected);
    assert_eq!((slice.open_start, slice.open_end), (open_start, open_end));
}

#[test]
fn slices_record_their_open_depths() {
    check_slice(
        doc(v![p(v!["hello<b> world"])]),
        doc(v![p(v!["hello"])]).node.content().clone(),
        0,
        1,
    );
    check_slice(
        doc(v![p(v!["hello<b>"])]),
        doc(v![p(v!["hello"])]).node.content().clone(),
        0,
        1,
    );
    check_slice(
        doc(v![p(v!["hello<b> world"]), p(v!["rest"])]),
        doc(v![p(v!["hello"])]).node.content().clone(),
        0,
        1,
    );
    check_slice(
        doc(v![p(v!["hello<a> world"])]),
        doc(v![p(v![" world"])]).node.content().clone(),
        1,
        0,
    );
    check_slice(
        doc(v![p(v!["foo<a>bar<b>baz"])]),
        p(v!["bar"]).node.content().clone(),
        0,
        0,
    );
    check_slice(
        doc(v![p(v!["a<a>b"]), p(v!["c<b>d"])]),
        doc(v![p(v!["b"]), p(v!["c"])]).node.content().clone(),
        1,
        1,
    );
    check_slice(
        doc(v![p(v!["a<a>b"]), p(v!["c"]), "<b>"]),
        doc(v![p(v!["b"]), p(v!["c"])]).node.content().clone(),
        1,
        0,
    );
}

// ---- replace ---------------------------------------------------------------

fn rpl(target: TaggedNode, insert: Option<TaggedNode>, expected: TaggedNode) {
    let slice = match &insert {
        Some(insert) => insert
            .node
            .slice(insert.tag("a"), insert.tag("b"), false)
            .unwrap(),
        None => Slice::empty(),
    };
    let result = target
        .node
        .replace(target.tag("a"), target.tag("b"), &slice)
        .unwrap();
    assert_eq!(result, expected.node, "{result} vs {}", expected.node);
    result.check().unwrap();
}

fn bad(target: TaggedNode, insert: Option<TaggedNode>, pattern: &str) {
    let slice = match &insert {
        Some(insert) => insert
            .node
            .slice(insert.tag("a"), insert.tag("b"), false)
            .unwrap(),
        None => Slice::empty(),
    };
    let error = target
        .node
        .replace(target.tag("a"), target.tag("b"), &slice)
        .expect_err("replace should fail");
    assert!(
        error.to_string().contains(pattern),
        "{error} does not mention {pattern}"
    );
}

#[test]
fn replace_joins_on_delete() {
    rpl(
        doc(v![p(v!["on<a>e"]), p(v!["t<b>wo"])]),
        None,
        doc(v![p(v!["onwo"])]),
    );
}

#[test]
fn replace_merges_matching_blocks() {
    rpl(
        doc(v![p(v!["on<a>e"]), p(v!["t<b>wo"])]),
        Some(doc(v![p(v!["xx<a>xx"]), p(v!["yy<b>yy"])])),
        doc(v![p(v!["onxx"]), p(v!["yywo"])]),
    );
}

#[test]
fn replace_merges_when_adding_text() {
    rpl(
        doc(v![p(v!["on<a>e"]), p(v!["t<b>wo"])]),
        Some(doc(v![p(v!["<a>H<b>"])])),
        doc(v![p(v!["onHwo"])]),
    );
}

#[test]
fn replace_can_insert_text() {
    rpl(
        doc(v![p(v!["before"]), p(v!["on<a><b>e"]), p(v!["after"])]),
        Some(doc(v![p(v!["<a>H<b>"])])),
        doc(v![p(v!["before"]), p(v!["onHe"]), p(v!["after"])]),
    );
}

#[test]
fn replace_does_not_merge_non_matching_blocks() {
    rpl(
        doc(v![p(v!["on<a>e"]), p(v!["t<b>wo"])]),
        Some(doc(v![h1(v!["<a>H<b>"])])),
        doc(v![p(v!["onHwo"])]),
    );
}

#[test]
fn replace_can_merge_a_nested_node() {
    rpl(
        doc(v![blockquote(v![blockquote(v![
            p(v!["on<a>e"]),
            p(v!["t<b>wo"])
        ])])]),
        Some(doc(v![p(v!["<a>H<b>"])])),
        doc(v![blockquote(v![blockquote(v![p(v!["onHwo"])])])]),
    );
}

#[test]
fn replace_can_replace_within_a_block() {
    rpl(
        doc(v![blockquote(v![p(v!["a<a>bc<b>d"])])]),
        Some(doc(v![p(v!["x<a>y<b>z"])])),
        doc(v![blockquote(v![p(v!["ayd"])])]),
    );
}

#[test]
fn replace_can_insert_a_lopsided_slice() {
    rpl(
        doc(v![blockquote(v![blockquote(v![
            p(v!["on<a>e"]),
            p(v!["two"]),
            "<b>",
            p(v!["three"])
        ])])]),
        Some(doc(v![blockquote(v![
            p(v!["aa<a>aa"]),
            p(v!["bb"]),
            p(v!["cc"]),
            "<b>",
            p(v!["dd"])
        ])])),
        doc(v![blockquote(v![blockquote(v![
            p(v!["onaa"]),
            p(v!["bb"]),
            p(v!["cc"]),
            p(v!["three"])
        ])])]),
    );
}

#[test]
fn replace_can_insert_a_deep_lopsided_slice() {
    rpl(
        doc(v![blockquote(v![
            blockquote(v![p(v!["on<a>e"]), p(v!["two"]), p(v!["three"])]),
            "<b>",
            p(v!["x"])
        ])]),
        Some(doc(v![
            blockquote(v![p(v!["aa<a>aa"]), p(v!["bb"]), p(v!["cc"])]),
            "<b>",
            p(v!["dd"])
        ])),
        doc(v![blockquote(v![
            blockquote(v![p(v!["onaa"]), p(v!["bb"]), p(v!["cc"])]),
            p(v!["x"])
        ])]),
    );
}

#[test]
fn replace_can_merge_multiple_levels() {
    rpl(
        doc(v![
            blockquote(v![blockquote(v![p(v!["hell<a>o"])])]),
            blockquote(v![blockquote(v![p(v!["<b>a"])])])
        ]),
        None,
        doc(v![blockquote(v![blockquote(v![p(v!["hella"])])])]),
    );
}

#[test]
fn replace_can_merge_multiple_levels_while_inserting() {
    rpl(
        doc(v![
            blockquote(v![blockquote(v![p(v!["hell<a>o"])])]),
            blockquote(v![blockquote(v![p(v!["<b>a"])])])
        ]),
        Some(doc(v![p(v!["<a>i<b>"])])),
        doc(v![blockquote(v![blockquote(v![p(v!["hellia"])])])]),
    );
}

#[test]
fn replace_can_insert_a_split() {
    rpl(
        doc(v![p(v!["foo<a><b>bar"])]),
        Some(doc(v![p(v!["<a>x"]), p(v!["y<b>"])])),
        doc(v![p(v!["foox"]), p(v!["ybar"])]),
    );
}

#[test]
fn replace_can_insert_a_deep_split() {
    rpl(
        doc(v![blockquote(v![p(v!["foo<a>x<b>bar"])])]),
        Some(doc(v![
            blockquote(v![p(v!["<a>x"])]),
            blockquote(v![p(v!["y<b>"])])
        ])),
        doc(v![
            blockquote(v![p(v!["foox"])]),
            blockquote(v![p(v!["ybar"])])
        ]),
    );
}

#[test]
fn replace_can_add_a_split_one_level_up() {
    rpl(
        doc(v![blockquote(v![p(v!["foo<a>u"]), p(v!["v<b>bar"])])]),
        Some(doc(v![
            blockquote(v![p(v!["<a>x"])]),
            blockquote(v![p(v!["y<b>"])])
        ])),
        doc(v![
            blockquote(v![p(v!["foox"])]),
            blockquote(v![p(v!["ybar"])])
        ]),
    );
}

#[test]
fn replace_keeps_the_node_type_of_the_left_node() {
    rpl(
        doc(v![h1(v!["foo<a>bar"]), "<b>"]),
        Some(doc(v![p(v!["foo<a>baz"]), "<b>"])),
        doc(v![h1(v!["foobaz"])]),
    );
}

#[test]
fn replace_keeps_the_node_type_even_when_empty() {
    rpl(
        doc(v![h1(v!["<a>bar"]), "<b>"]),
        Some(doc(v![p(v!["foo<a>baz"]), "<b>"])),
        doc(v![h1(v!["baz"])]),
    );
}

#[test]
fn replace_rejects_a_left_side_that_is_too_deep() {
    bad(
        doc(v![p(v!["<a><b>"])]),
        Some(doc(v![blockquote(v![p(v!["<a>"])]), "<b>"])),
        "deeper",
    );
}

#[test]
fn replace_rejects_a_depth_mismatch() {
    bad(
        doc(v![p(v!["<a><b>"])]),
        Some(doc(v!["<a>", p(v!["<b>"])])),
        "inconsistent",
    );
}

#[test]
fn replace_rejects_a_bad_fit() {
    bad(
        doc(v!["<a>", "<b>"]),
        Some(doc(v![p(v!["<a>foo<b>"])])),
        "invalid content",
    );
}

#[test]
fn replace_rejects_unjoinable_content() {
    bad(
        doc(v![ul(v![li(v![p(v!["a"])]), "<a>"]), "<b>"]),
        Some(doc(v![p(v!["foo", "<a>"]), "<b>"])),
        "cannot join",
    );
}

#[test]
fn replace_rejects_an_unjoinable_delete() {
    bad(
        doc(v![
            blockquote(v![p(v!["a"]), "<a>"]),
            ul(v!["<b>", li(v![p(v!["b"])])])
        ]),
        None,
        "cannot join",
    );
}

#[test]
fn replace_checks_content_validity() {
    bad(
        doc(v![blockquote(v!["<a>", p(v!["hi"])]), "<b>"]),
        Some(doc(v![blockquote(v!["hi", "<a>"]), "<b>"])),
        "invalid content",
    );
}

// ---- content expressions ---------------------------------------------------

fn probe(expr: &str) -> Schema {
    Schema::new(schema_spec().node("probe", NodeSpec::new().content(expr))).unwrap()
}

fn matches(expr: &str, types: &str) -> bool {
    let schema = probe(expr);
    let mut state = Some(schema.node_type("probe").unwrap().content_match());
    for name in types.split_whitespace() {
        let ty = schema.node_type(name).unwrap();
        state = state.and_then(|state| state.match_type(&ty));
    }
    state.is_some_and(|state| state.valid_end())
}

#[test]
fn content_expressions_accept_and_reject_sequences() {
    let cases: &[(&str, &str, bool)] = &[
        ("", "", true),
        ("", "image", false),
        ("image*", "", true),
        ("image*", "image", true),
        ("image*", "image image image image", true),
        ("image*", "image text", false),
        ("inline*", "image text", true),
        ("(paragraph | heading)", "paragraph", true),
        ("(paragraph | heading)", "horizontal_rule", false),
        (
            "paragraph horizontal_rule paragraph",
            "paragraph horizontal_rule paragraph",
            true,
        ),
        (
            "paragraph horizontal_rule paragraph",
            "paragraph horizontal_rule",
            false,
        ),
        (
            "paragraph horizontal_rule paragraph",
            "paragraph horizontal_rule horizontal_rule",
            false,
        ),
        ("heading paragraph*", "heading", true),
        ("heading paragraph*", "heading paragraph paragraph", true),
        ("heading paragraph+", "heading paragraph", true),
        ("heading paragraph+", "heading", false),
        ("heading paragraph+", "paragraph paragraph", false),
        ("image?", "image", true),
        ("image?", "", true),
        ("image?", "image image", false),
        (
            "(heading paragraph+)+",
            "heading paragraph heading paragraph paragraph",
            true,
        ),
        (
            "(heading paragraph+)+",
            "heading paragraph heading paragraph paragraph horizontal_rule",
            false,
        ),
        ("hard_break{2}", "hard_break hard_break", true),
        ("hard_break{2}", "hard_break", false),
        ("hard_break{2}", "hard_break hard_break hard_break", false),
        ("hard_break{2, 4}", "hard_break hard_break", true),
        (
            "hard_break{2, 4}",
            "hard_break hard_break hard_break hard_break",
            true,
        ),
        (
            "hard_break{2, 4}",
            "hard_break hard_break hard_break hard_break hard_break",
            false,
        ),
        (
            "hard_break{2,}",
            "hard_break hard_break hard_break hard_break hard_break",
            true,
        ),
        ("hard_break{2,}", "hard_break", false),
    ];
    for (expr, types, valid) in cases {
        assert_eq!(matches(expr, types), *valid, "{expr:?} with {types:?}");
    }
}

#[test]
fn content_expressions_report_schema_errors() {
    let mixed = Schema::new(schema_spec().node("probe", NodeSpec::new().content("paragraph text")));
    assert!(
        mixed
            .unwrap_err()
            .to_string()
            .contains("mixing inline and block")
    );
    let unknown = Schema::new(schema_spec().node("probe", NodeSpec::new().content("nope+")));
    assert!(unknown.unwrap_err().to_string().contains("nope"));
    let dead = Schema::new(schema_spec().node("probe", NodeSpec::new().content("image")));
    assert!(dead.unwrap_err().to_string().contains("non-generatable"));
}

#[test]
fn fill_before_generates_missing_nodes() {
    let schema = probe("paragraph horizontal_rule paragraph");
    let start = schema.node_type("probe").unwrap().content_match();
    let after = doc(v![p(v![])]).node.content().clone();
    let before = doc(v![p(v![])]).node.content().clone();
    let state = start
        .match_fragment(&before, 0, before.child_count())
        .unwrap();
    let filled = state.fill_before(&after, true, 0).unwrap();
    assert_eq!(filled.to_string(), "horizontal_rule");

    let breaks = probe("hard_break+");
    let start = breaks.node_type("probe").unwrap().content_match();
    assert_eq!(
        start
            .fill_before(&Fragment::empty(), true, 0)
            .unwrap()
            .to_string(),
        "hard_break"
    );
}

#[test]
fn find_wrapping_finds_list_wrappers() {
    let schema = schema();
    let doc_match = schema.node_type("doc").unwrap().content_match();
    let li = schema.node_type("list_item").unwrap();
    let wrapping = doc_match.find_wrapping(&li).unwrap();
    assert_eq!(
        wrapping.iter().map(|ty| ty.name()).collect::<Vec<_>>(),
        ["ordered_list"]
    );
    let p = schema.node_type("paragraph").unwrap();
    assert!(doc_match.find_wrapping(&p).unwrap().is_empty());
}

// ---- marks -----------------------------------------------------------------

#[test]
fn mark_sets_stay_ordered_and_respect_exclusion() {
    let schema = schema();
    let em = schema.mark_type("em").unwrap().create(None).unwrap();
    let strong = schema.mark_type("strong").unwrap().create(None).unwrap();
    let link = |href: &str| {
        schema
            .mark_type("link")
            .unwrap()
            .create(Some(json!({"href": href}).as_object().unwrap()))
            .unwrap()
    };
    let set = strong.add_to_set(&em.add_to_set(&MarkSet::none()));
    assert_eq!(format!("{set:?}"), "[em, strong]");
    let set = link("a").add_to_set(&set);
    assert_eq!(set[0], link("a"));
    let replaced = link("b").add_to_set(&set);
    assert_eq!(replaced.len(), 3, "a link excludes another link");
    assert_eq!(replaced[0], link("b"));
    assert!(em.add_to_set(&set).same_set(&set));
    assert_eq!(
        format!("{:?}", em.remove_from_set(&set)),
        format!("{:?}", MarkSet::from_marks([link("a"), strong.clone()]))
    );
}

#[test]
fn resolved_marks_drop_non_inclusive_marks_at_their_end() {
    let d = doc(v![p(v![a(v!["link<a>"]), "<b>x"])]);
    let at_end = d.node.resolve(d.tag("a")).unwrap();
    assert!(
        at_end.marks().is_empty(),
        "typing after a link does not extend it"
    );
    let inside = doc(v![p(v![a(v!["li<a>nk"])])]);
    assert_eq!(
        inside.node.resolve(inside.tag("a")).unwrap().marks().len(),
        1
    );
    let emphasis = doc(v![p(v![em(v!["word<a>"]), " x"])]);
    assert_eq!(
        emphasis
            .node
            .resolve(emphasis.tag("a"))
            .unwrap()
            .marks()
            .len(),
        1
    );
}

// ---- nodes -----------------------------------------------------------------

#[test]
fn nodes_print_like_prosemirror() {
    let d = doc(v![ul(v![
        li(v![p(v!["hey"]), p(v![])]),
        li(v![p(v!["foo"])])
    ])])
    .node;
    assert_eq!(
        d.to_string(),
        r#"doc(bullet_list(list_item(paragraph("hey"), paragraph), list_item(paragraph("foo"))))"#
    );
    let d = doc(v![p(v!["foo", img(), br(), "bar"])]).node;
    assert_eq!(
        d.to_string(),
        r#"doc(paragraph("foo", image, hard_break, "bar"))"#
    );
    let d = doc(v![p(v![em(v!["hello"]), strong(v![em(v!["world"])])])]).node;
    assert_eq!(
        d.to_string(),
        r#"doc(paragraph(em("hello"), em(strong("world"))))"#
    );
}

#[test]
fn text_between_uses_separators_and_leaf_text() {
    let d = doc(v![p(v!["foo", br(), "bar"]), p(v!["baz"])]).node;
    assert_eq!(
        d.text_between(0, d.content().size(), Some("|"), None),
        "foo\nbar|baz"
    );
    assert_eq!(d.text_between(2, 6, None, Some("*")), "oo*b");
}

#[test]
fn json_round_trips_and_validates() {
    let d = doc(v![
        h2(v!["Title"]),
        p(v![
            "plain ",
            strong(v!["bold ", em(v!["both"])]),
            a(v!["link"])
        ]),
        ul(v![li(v![p(v!["item"])])])
    ])
    .node;
    let value = d.to_json();
    assert_eq!(
        value["content"][0],
        json!({"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "Title"}]})
    );
    let schema = schema();
    assert_eq!(schema.node_from_json(&value).unwrap(), d);

    let invalid = json!({"type": "doc", "content": [{"type": "text", "text": "loose"}]});
    assert_eq!(
        schema.node_from_json(&invalid).unwrap_err(),
        ModelError::InvalidContent("doc".into())
    );
    let unknown = json!({"type": "doc", "content": [{"type": "table"}]});
    assert_eq!(
        schema.node_from_json(&unknown).unwrap_err(),
        ModelError::UnknownNodeType("table".into())
    );
    let bad_attr =
        json!({"type": "doc", "content": [{"type": "heading", "attrs": {"level": "one"}}]});
    assert!(matches!(
        schema.node_from_json(&bad_attr).unwrap_err(),
        ModelError::InvalidAttr { .. }
    ));
    let missing =
        json!({"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "image"}]}]});
    assert!(matches!(
        schema.node_from_json(&missing).unwrap_err(),
        ModelError::MissingAttr { .. }
    ));
}

#[test]
fn create_and_fill_completes_required_content() {
    let schema = schema();
    let list = schema.node_type("bullet_list").unwrap();
    let filled = list
        .create_and_fill(None, Fragment::empty(), MarkSet::none())
        .unwrap();
    assert_eq!(filled.to_string(), "bullet_list(list_item(paragraph))");
    let d = schema.node_type("doc").unwrap();
    assert_eq!(
        d.create_and_fill(None, Fragment::empty(), MarkSet::none())
            .unwrap()
            .to_string(),
        "doc(paragraph)"
    );
}

#[test]
fn can_replace_checks_the_content_expression() {
    let d = doc(v![p(v!["a"]), p(v!["b"])]).node;
    let paragraph = d.child(0).clone();
    assert!(d.can_replace(0, 1, &Fragment::empty(), 0, 0));
    assert!(
        !d.can_replace(0, 2, &Fragment::empty(), 0, 0),
        "doc needs a block"
    );
    assert!(d.can_replace_with(0, 2, paragraph.node_type(), None));
    let text = schema().text("x", MarkSet::none()).unwrap();
    assert!(!d.can_replace_with(0, 0, text.node_type(), None));
}

#[test]
fn indexed_fragments_find_the_same_children_as_a_scan() {
    // Paragraphs of varying sizes, enough to use the offset index.
    let blocks: Vec<Item> = (0..80)
        .map(|i| {
            if i % 3 == 0 {
                p(v![]).into()
            } else {
                p(v!["x".repeat(i % 7 + 1).as_str()]).into()
            }
        })
        .collect();
    let test_doc = doc(blocks).node;
    let content = test_doc.content();
    let small: Vec<Node> = content.iter().cloned().collect();
    for pos in 0..=content.size() {
        for round in [-1, 1] {
            let indexed = content.find_index(pos, round).unwrap();
            // Linear reference.
            let mut start = 0;
            let mut expected = (small.len(), pos);
            for (index, child) in small.iter().enumerate() {
                let end = start + child.node_size();
                if pos == 0 {
                    expected = (0, 0);
                    break;
                }
                if end >= pos {
                    expected = if end == pos || round > 0 {
                        (index + 1, end)
                    } else {
                        (index, start)
                    };
                    break;
                }
                start = end;
            }
            assert_eq!(
                (indexed.index, indexed.offset),
                expected,
                "pos {pos} round {round}"
            );
        }
    }
    assert!(test_doc.resolve(content.size() / 2).is_ok());
}
