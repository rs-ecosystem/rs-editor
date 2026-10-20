//! Port of prosemirror-transform's `test-trans.ts` and `test-mapping.ts`.
//! Every transform is also checked for inversion, step JSON round-trips
//! and tag mapping, like ProseMirror's `testTransform`.

use rs_editor_model::testing::*;
use rs_editor_model::{
    AttrSpec, Fragment, Mark, MarkSet, MarkSpec, Node, NodeSpec, Schema, SchemaSpec, Slice, v,
};
use serde_json::{Value, json};

use crate::*;

fn mark(name: &str) -> Mark {
    schema().mark_type(name).unwrap().create(None).unwrap()
}

fn mark_with(name: &str, attrs: Value) -> Mark {
    schema()
        .mark_type(name)
        .unwrap()
        .create(attrs.as_object())
        .unwrap()
}

fn node(name: &str, content: Vec<Node>) -> Node {
    schema()
        .node_type(name)
        .unwrap()
        .create(None, Fragment::from_array(content), MarkSet::none())
        .unwrap()
}

fn text(value: &str) -> Node {
    schema().text(value, MarkSet::none()).unwrap()
}

fn inline(item: Item) -> Node {
    p(vec![item]).node.child(0).clone()
}

fn block(item: Item) -> Node {
    doc(vec![item]).node.child(0).clone()
}

fn tag_or_a(tagged: &TaggedNode, name: &str) -> usize {
    tagged
        .tags
        .get(name)
        .copied()
        .unwrap_or_else(|| tagged.tag("a"))
}

fn invert(tr: &Transform) -> Transform {
    let mut out = Transform::new(tr.doc.clone());
    for i in (0..tr.steps.len()).rev() {
        out.step(tr.steps[i].invert(&tr.docs[i]).unwrap()).unwrap();
    }
    out
}

fn check_mapping(mapping: &Mapping, pos: usize, new_pos: usize) {
    assert_eq!(mapping.map(pos, Assoc::After), new_pos, "mapping {pos}");
    let maps = mapping.maps();
    let mut remap = Mapping::from_maps(maps.iter().map(StepMap::invert).collect());
    let mut map_from = maps.len();
    for map in maps.iter().rev() {
        map_from -= 1;
        remap.append_map(map.clone(), Some(map_from));
    }
    assert_eq!(remap.map(pos, Assoc::After), pos, "remapping {pos}");
}

fn check_step_json(tr: &Transform) {
    let schema = tr.doc.node_type().schema().clone();
    let mut replay = Transform::new(tr.before().clone());
    for step in &tr.steps {
        replay
            .step(Step::from_json(&schema, &step.to_json()).unwrap())
            .unwrap();
    }
    assert_eq!(tr.doc, replay.doc);
}

fn test_transform(tr: &Transform, before: &TaggedNode, expect: &TaggedNode) {
    assert_eq!(
        tr.doc, expect.node,
        "\n  got  {}\n  want {}",
        tr.doc, expect.node
    );
    assert_eq!(invert(tr).doc, *tr.before(), "inversion");
    check_step_json(tr);
    for (tag, pos) in &expect.tags {
        let old = before
            .tags
            .get(tag)
            .unwrap_or_else(|| panic!("tag <{tag}> missing before"));
        check_mapping(&tr.mapping, *old, *pos);
    }
}

fn check_add(d: TaggedNode, mark: Mark, expect: TaggedNode) {
    let mut tr = Transform::new(d.node.clone());
    tr.add_mark(d.tag("a"), d.tag("b"), &mark).unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_rem(d: TaggedNode, matching: MarkMatch, expect: TaggedNode) {
    let mut tr = Transform::new(d.node.clone());
    tr.remove_mark(d.tag("a"), d.tag("b"), &matching).unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_ins(d: TaggedNode, nodes: Vec<Node>, expect: TaggedNode) {
    let mut tr = Transform::new(d.node.clone());
    tr.insert(d.tag("a"), Fragment::from_array(nodes)).unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_del(d: TaggedNode, expect: TaggedNode) {
    let mut tr = Transform::new(d.node.clone());
    tr.delete(d.tag("a"), d.tag("b")).unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_join(d: TaggedNode, expect: TaggedNode) {
    let mut tr = Transform::new(d.node.clone());
    tr.join(d.tag("a"), 1).unwrap();
    test_transform(&tr, &d, &expect);
}

fn block_range(d: &TaggedNode) -> rs_editor_model::NodeRange {
    let from = d.node.resolve(d.tag("a")).unwrap();
    let to = d.node.resolve(tag_or_a(d, "b")).unwrap();
    from.block_range(&to, None).unwrap()
}

fn check_lift(d: TaggedNode, expect: TaggedNode) {
    let range = block_range(&d);
    let mut tr = Transform::new(d.node.clone());
    tr.lift(&range, lift_target(&range).unwrap()).unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_wrap(d: TaggedNode, expect: TaggedNode, ty: &str) {
    let range = block_range(&d);
    let wrappers = find_wrapping(&range, &schema().node_type(ty).unwrap(), None, None).unwrap();
    let mut tr = Transform::new(d.node.clone());
    tr.wrap(&range, &wrappers).unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_type(d: TaggedNode, expect: TaggedNode, ty: &str, attrs: Option<Value>) {
    let ty = d.node.node_type().schema().node_type(ty).unwrap();
    let mut tr = Transform::new(d.node.clone());
    tr.set_block_type(
        d.tag("a"),
        tag_or_a(&d, "b"),
        &ty,
        attrs.as_ref().and_then(Value::as_object),
    )
    .unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_markup(d: TaggedNode, expect: TaggedNode, ty: &str, attrs: Value) {
    let ty = schema().node_type(ty).unwrap();
    let mut tr = Transform::new(d.node.clone());
    tr.set_node_markup(d.tag("a"), Some(&ty), attrs.as_object(), None)
        .unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_repl_slice(d: TaggedNode, slice: Slice, expect: TaggedNode) {
    let mut tr = Transform::new(d.node.clone());
    tr.replace(d.tag("a"), tag_or_a(&d, "b"), slice).unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_repl(d: TaggedNode, source: Option<TaggedNode>, expect: TaggedNode) {
    let slice = match source {
        Some(source) => source
            .node
            .slice(source.tag("a"), source.tag("b"), false)
            .unwrap(),
        None => Slice::empty(),
    };
    check_repl_slice(d, slice, expect);
}

fn check_repl_range(d: TaggedNode, source: TaggedNode, expect: TaggedNode) {
    let slice = source
        .node
        .slice(source.tag("a"), source.tag("b"), true)
        .unwrap();
    let mut tr = Transform::new(d.node.clone());
    tr.replace_range(d.tag("a"), tag_or_a(&d, "b"), slice)
        .unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_repl_range_with(d: TaggedNode, node: Node, expect: TaggedNode) {
    let mut tr = Transform::new(d.node.clone());
    tr.replace_range_with(d.tag("a"), tag_or_a(&d, "b"), node)
        .unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_delete_range(d: TaggedNode, expect: TaggedNode) {
    let mut tr = Transform::new(d.node.clone());
    tr.delete_range(d.tag("a"), tag_or_a(&d, "b")).unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_add_node_mark(d: TaggedNode, mark: Mark, expect: TaggedNode) {
    let mut tr = Transform::new(d.node.clone());
    tr.add_node_mark(d.tag("a"), mark).unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_rm_node_mark(d: TaggedNode, matching: MarkMatch, expect: TaggedNode) {
    let mut tr = Transform::new(d.node.clone());
    tr.remove_node_mark(d.tag("a"), &matching).unwrap();
    test_transform(&tr, &d, &expect);
}

fn check_set_attr(d: TaggedNode, attr: &str, value: Value, expect: TaggedNode) {
    let mut tr = Transform::new(d.node.clone());
    tr.set_node_attribute(d.tag("a"), attr, value).unwrap();
    test_transform(&tr, &d, &expect);
}

// ---- converted from test-trans.ts ------------------------------------------

#[test]
fn addmark_should_add_a_mark() {
    check_add(
        doc(v![p(v!["hello <a>there<b>!"])]),
        mark("strong"),
        doc(v![p(v!["hello ", strong(v!["there"]), "!"])]),
    );
}

#[test]
fn addmark_should_only_add_a_mark_once() {
    check_add(
        doc(v![p(v!["hello ", strong(v!["<a>there"]), "!<b>"])]),
        mark("strong"),
        doc(v![p(v!["hello ", strong(v!["there!"])])]),
    );
}

#[test]
fn addmark_should_join_overlapping_marks() {
    check_add(
        doc(v![p(v!["one <a>two ", em(v!["three<b> four"])])]),
        mark("strong"),
        doc(v![p(v![
            "one ",
            strong(v!["two ", em(v!["three"])]),
            em(v![" four"])
        ])]),
    );
}

#[test]
fn addmark_should_overwrite_marks_with_different_attributes() {
    check_add(
        doc(v![p(v!["this is a ", a(v!["<a>link<b>"])])]),
        mark_with("link", json!({"href": "bar"})),
        doc(v![p(v![
            "this is a ",
            m("link", json!({"href": "bar"}), v!["link"])
        ])]),
    );
}

#[test]
fn addmark_can_add_a_mark_in_a_nested_node() {
    check_add(
        doc(v![
            p(v!["before"]),
            blockquote(v![p(v!["the variable is called <a>i<b>"])]),
            p(v!["after"])
        ]),
        mark("code"),
        doc(v![
            p(v!["before"]),
            blockquote(v![p(v!["the variable is called ", code(v!["i"])])]),
            p(v!["after"])
        ]),
    );
}

#[test]
fn addmark_can_add_a_mark_across_blocks() {
    check_add(
        doc(v![
            p(v!["hi <a>this"]),
            blockquote(v![p(v!["is"])]),
            p(v!["a docu<b>ment"]),
            p(v!["!"])
        ]),
        mark("em"),
        doc(v![
            p(v!["hi ", em(v!["this"])]),
            blockquote(v![p(v![em(v!["is"])])]),
            p(v![em(v!["a docu"]), "ment"]),
            p(v!["!"])
        ]),
    );
}

#[test]
fn removemark_can_cut_a_gap() {
    check_rem(
        doc(v![p(v![em(v!["hello <a>world<b>!"])])]),
        MarkMatch::Mark(mark("em")),
        doc(v![p(v![em(v!["hello "]), "world", em(v!["!"])])]),
    );
}

#[test]
fn removemark_doesn_t_do_anything_when_there_s_no_mark() {
    check_rem(
        doc(v![p(v![em(v!["hello"]), " <a>world<b>!"])]),
        MarkMatch::Mark(mark("em")),
        doc(v![p(v![em(v!["hello"]), " <a>world<b>!"])]),
    );
}

#[test]
fn removemark_can_remove_marks_from_nested_nodes() {
    check_rem(
        doc(v![p(v![em(v!["one ", strong(v!["<a>two<b>"]), " three"])])]),
        MarkMatch::Mark(mark("strong")),
        doc(v![p(v![em(v!["one two three"])])]),
    );
}

#[test]
fn removemark_can_remove_a_link() {
    check_rem(
        doc(v![p(v!["<a>hello ", a(v!["link<b>"])])]),
        MarkMatch::Mark(mark_with("link", json!({"href": "foo"}))),
        doc(v![p(v!["hello link"])]),
    );
}

#[test]
fn removemark_doesn_t_remove_a_non_matching_link() {
    check_rem(
        doc(v![p(v!["<a>hello ", a(v!["link<b>"])])]),
        MarkMatch::Mark(mark_with("link", json!({"href": "bar"}))),
        doc(v![p(v!["hello ", a(v!["link"])])]),
    );
}

#[test]
fn removemark_can_remove_across_blocks() {
    check_rem(
        doc(v![
            blockquote(v![p(v![em(v!["much <a>em"])]), p(v![em(v!["here too"])])]),
            p(v!["between", em(v!["..."])]),
            p(v![em(v!["end<b>"])])
        ]),
        MarkMatch::Mark(mark("em")),
        doc(v![
            blockquote(v![p(v![em(v!["much "]), "em"]), p(v!["here too"])]),
            p(v!["between..."]),
            p(v!["end"])
        ]),
    );
}

#[test]
fn removemark_can_remove_everything() {
    check_rem(
        doc(v![p(v![
            "<a>hello, ",
            em(v!["this is ", strong(v!["much"]), " ", a(v!["markup<b>"])])
        ])]),
        MarkMatch::All,
        doc(v![p(v!["<a>hello, this is much markup"])]),
    );
}

#[test]
fn insert_can_insert_a_break() {
    check_ins(
        doc(v![p(v!["hello<a>there"])]),
        vec![node("hard_break", vec![])],
        doc(v![p(v!["hello", br(), "<a>there"])]),
    );
}

#[test]
fn insert_can_insert_an_empty_paragraph_at_the_top() {
    check_ins(
        doc(v![p(v!["one"]), "<a>", p(v!["two<2>"])]),
        vec![node("paragraph", vec![])],
        doc(v![p(v!["one"]), p(v![]), "<a>", p(v!["two<2>"])]),
    );
}

#[test]
fn insert_can_insert_two_block_nodes() {
    check_ins(
        doc(v![p(v!["one"]), "<a>", p(v!["two<2>"])]),
        vec![
            node("paragraph", vec![text("hi")]),
            node("horizontal_rule", vec![]),
        ],
        doc(v![p(v!["one"]), p(v!["hi"]), hr(), "<a>", p(v!["two<2>"])]),
    );
}

#[test]
fn insert_can_insert_at_the_end_of_a_blockquote() {
    check_ins(
        doc(v![
            blockquote(v![p(v!["he<before>y"]), "<a>"]),
            p(v!["after<after>"])
        ]),
        vec![node("paragraph", vec![])],
        doc(v![
            blockquote(v![p(v!["he<before>y"]), p(v![])]),
            p(v!["after<after>"])
        ]),
    );
}

#[test]
fn insert_can_insert_at_the_start_of_a_blockquote() {
    check_ins(
        doc(v![
            blockquote(v!["<a>", p(v!["he<1>y"])]),
            p(v!["after<2>"])
        ]),
        vec![node("paragraph", vec![])],
        doc(v![
            blockquote(v![p(v![]), "<a>", p(v!["he<1>y"])]),
            p(v!["after<2>"])
        ]),
    );
}

#[test]
fn insert_will_wrap_a_node_with_the_suitable_parent() {
    check_ins(
        doc(v![p(v!["foo<a>bar"])]),
        vec![
            schema()
                .node_type("list_item")
                .unwrap()
                .create_and_fill(None, Fragment::empty(), MarkSet::none())
                .unwrap(),
        ],
        doc(v![p(v!["foo"]), ol(v![li(v![p(v![])])]), p(v!["bar"])]),
    );
}

#[test]
fn delete_can_delete_a_word() {
    check_del(
        doc(v![
            p(v!["<1>one"]),
            "<a>",
            p(v!["tw<2>o"]),
            "<b>",
            p(v!["<3>three"])
        ]),
        doc(v![p(v!["<1>one"]), "<a><2>", p(v!["<3>three"])]),
    );
}

#[test]
fn delete_preserves_content_constraints() {
    check_del(
        doc(v![blockquote(v!["<a>", p(v!["hi"]), "<b>"]), p(v!["x"])]),
        doc(v![blockquote(v![p(v![])]), p(v!["x"])]),
    );
}

#[test]
fn delete_preserves_positions_after_the_range() {
    check_del(
        doc(v![
            blockquote(v![p(v!["a"]), "<a>", p(v!["b"]), "<b>"]),
            p(v!["c<1>"])
        ]),
        doc(v![blockquote(v![p(v!["a"])]), p(v!["c<1>"])]),
    );
}

#[test]
fn delete_doesn_t_join_incompatible_nodes() {
    check_del(
        doc(v![pre(v!["fo<a>o"]), p(v!["b<b>ar", img()])]),
        doc(v![pre(v!["fo"]), p(v!["ar", img()])]),
    );
}

#[test]
fn delete_doesn_t_join_when_marks_are_incompatible() {
    check_del(
        doc(v![pre(v!["fo<a>o"]), p(v![em(v!["b<b>ar"])])]),
        doc(v![pre(v!["fo"]), p(v![em(v!["ar"])])]),
    );
}

#[test]
fn join_can_join_blocks() {
    check_join(
        doc(v![
            blockquote(v![p(v!["<before>a"])]),
            "<a>",
            blockquote(v![p(v!["b"])]),
            p(v!["after<after>"])
        ]),
        doc(v![
            blockquote(v![p(v!["<before>a"]), "<a>", p(v!["b"])]),
            p(v!["after<after>"])
        ]),
    );
}

#[test]
fn join_can_join_compatible_blocks() {
    check_join(
        doc(v![h1(v!["foo"]), "<a>", p(v!["bar"])]),
        doc(v![h1(v!["foobar"])]),
    );
}

#[test]
fn join_can_join_nested_blocks() {
    check_join(
        doc(v![blockquote(v![
            blockquote(v![p(v!["a"]), p(v!["b<before>"])]),
            "<a>",
            blockquote(v![p(v!["c"]), p(v!["d<after>"])])
        ])]),
        doc(v![blockquote(v![blockquote(v![
            p(v!["a"]),
            p(v!["b<before>"]),
            "<a>",
            p(v!["c"]),
            p(v!["d<after>"])
        ])])]),
    );
}

#[test]
fn join_can_join_lists() {
    check_join(
        doc(v![
            ol(v![li(v![p(v!["one"])]), li(v![p(v!["two"])])]),
            "<a>",
            ol(v![li(v![p(v!["three"])])])
        ]),
        doc(v![ol(v![
            li(v![p(v!["one"])]),
            li(v![p(v!["two"])]),
            "<a>",
            li(v![p(v!["three"])])
        ])]),
    );
}

#[test]
fn join_can_join_list_items() {
    check_join(
        doc(v![ol(v![
            li(v![p(v!["one"])]),
            li(v![p(v!["two"])]),
            "<a>",
            li(v![p(v!["three"])])
        ])]),
        doc(v![ol(v![
            li(v![p(v!["one"])]),
            li(v![p(v!["two"]), "<a>", p(v!["three"])])
        ])]),
    );
}

#[test]
fn join_can_join_textblocks() {
    check_join(
        doc(v![p(v!["foo"]), "<a>", p(v!["bar"])]),
        doc(v![p(v!["foo<a>bar"])]),
    );
}

#[test]
fn lift_can_lift_a_block_out_of_the_middle_of_its_parent() {
    check_lift(
        doc(v![blockquote(v![
            p(v!["<before>one"]),
            p(v!["<a>two"]),
            p(v!["<after>three"])
        ])]),
        doc(v![
            blockquote(v![p(v!["<before>one"])]),
            p(v!["<a>two"]),
            blockquote(v![p(v!["<after>three"])])
        ]),
    );
}

#[test]
fn lift_can_lift_a_block_from_the_start_of_its_parent() {
    check_lift(
        doc(v![blockquote(v![p(v!["<a>two"]), p(v!["<after>three"])])]),
        doc(v![p(v!["<a>two"]), blockquote(v![p(v!["<after>three"])])]),
    );
}

#[test]
fn lift_can_lift_a_block_from_the_end_of_its_parent() {
    check_lift(
        doc(v![blockquote(v![p(v!["<before>one"]), p(v!["<a>two"])])]),
        doc(v![blockquote(v![p(v!["<before>one"])]), p(v!["<a>two"])]),
    );
}

#[test]
fn lift_can_lift_a_single_child() {
    check_lift(
        doc(v![blockquote(v![p(v!["<a>t<in>wo"])])]),
        doc(v![p(v!["<a>t<in>wo"])]),
    );
}

#[test]
fn lift_can_lift_multiple_blocks() {
    check_lift(
        doc(v![blockquote(v![
            blockquote(v![p(v!["on<a>e"]), p(v!["tw<b>o"])]),
            p(v!["three"])
        ])]),
        doc(v![blockquote(v![
            p(v!["on<a>e"]),
            p(v!["tw<b>o"]),
            p(v!["three"])
        ])]),
    );
}

#[test]
fn lift_finds_a_valid_range_from_a_lopsided_selection() {
    check_lift(
        doc(v![
            p(v!["start"]),
            blockquote(v![blockquote(v![p(v!["a"]), p(v!["<a>b"])]), p(v!["<b>c"])])
        ]),
        doc(v![
            p(v!["start"]),
            blockquote(v![p(v!["a"]), p(v!["<a>b"])]),
            p(v!["<b>c"])
        ]),
    );
}

#[test]
fn lift_can_lift_from_a_nested_node() {
    check_lift(
        doc(v![blockquote(v![blockquote(v![
            p(v!["<1>one"]),
            p(v!["<a>two"]),
            p(v!["<3>three"]),
            p(v!["<b>four"]),
            p(v!["<5>five"])
        ])])]),
        doc(v![blockquote(v![
            blockquote(v![p(v!["<1>one"])]),
            p(v!["<a>two"]),
            p(v!["<3>three"]),
            p(v!["<b>four"]),
            blockquote(v![p(v!["<5>five"])])
        ])]),
    );
}

#[test]
fn lift_can_lift_from_a_list() {
    check_lift(
        doc(v![ul(v![
            li(v![p(v!["one"])]),
            li(v![p(v!["two<a>"])]),
            li(v![p(v!["three"])])
        ])]),
        doc(v![
            ul(v![li(v![p(v!["one"])])]),
            p(v!["two<a>"]),
            ul(v![li(v![p(v!["three"])])])
        ]),
    );
}

#[test]
fn lift_can_lift_from_the_end_of_a_list() {
    check_lift(
        doc(v![ul(v![li(v![p(v!["a"])]), li(v![p(v!["b<a>"])]), "<1>"])]),
        doc(v![ul(v![li(v![p(v!["a"])])]), p(v!["b<a>"]), "<1>"]),
    );
}

#[test]
fn wrap_can_wrap_in_a_blockquote() {
    check_wrap(
        doc(v![p(v!["one"]), p(v!["<a>two"]), p(v!["three"])]),
        doc(v![
            p(v!["one"]),
            blockquote(v![p(v!["<a>two"])]),
            p(v!["three"])
        ]),
        "blockquote",
    );
}

#[test]
fn wrap_can_wrap_two_paragraphs() {
    check_wrap(
        doc(v![
            p(v!["one<1>"]),
            p(v!["<a>two"]),
            p(v!["<b>three"]),
            p(v!["four<4>"])
        ]),
        doc(v![
            p(v!["one<1>"]),
            blockquote(v![p(v!["<a>two"]), p(v!["three"])]),
            p(v!["four<4>"])
        ]),
        "blockquote",
    );
}

#[test]
fn wrap_can_wrap_in_a_list() {
    check_wrap(
        doc(v![p(v!["<a>one"]), p(v!["<b>two"])]),
        doc(v![ol(v![li(v![p(v!["<a>one"]), p(v!["<b>two"])])])]),
        "ordered_list",
    );
}

#[test]
fn wrap_can_wrap_in_a_nested_list() {
    check_wrap(
        doc(v![ol(v![
            li(v![p(v!["<1>one"])]),
            li(v![p(v!["..."]), p(v!["<a>two"]), p(v!["<b>three"])]),
            li(v![p(v!["<4>four"])])
        ])]),
        doc(v![ol(v![
            li(v![p(v!["<1>one"])]),
            li(v![
                p(v!["..."]),
                ol(v![li(v![p(v!["<a>two"]), p(v!["<b>three"])])])
            ]),
            li(v![p(v!["<4>four"])])
        ])]),
        "ordered_list",
    );
}

#[test]
fn wrap_includes_half_covered_parent_nodes() {
    check_wrap(
        doc(v![
            blockquote(v![p(v!["<1>one"]), p(v!["two<a>"])]),
            p(v!["three<b>"])
        ]),
        doc(v![blockquote(v![
            blockquote(v![p(v!["<1>one"]), p(v!["two<a>"])]),
            p(v!["three<b>"])
        ])]),
        "blockquote",
    );
}

#[test]
fn setblocktype_can_change_a_single_textblock() {
    check_type(
        doc(v![p(v!["am<a> i"])]),
        doc(v![h2(v!["am i"])]),
        "heading",
        Some(json!({"level": 2})),
    );
}

#[test]
fn setblocktype_can_change_multiple_blocks() {
    check_type(
        doc(v![
            h1(v!["<a>hello"]),
            p(v!["there"]),
            p(v!["<b>you"]),
            p(v!["end"])
        ]),
        doc(v![
            pre(v!["hello"]),
            pre(v!["there"]),
            pre(v!["you"]),
            p(v!["end"])
        ]),
        "code_block",
        None,
    );
}

#[test]
fn setblocktype_can_change_a_wrapped_block() {
    check_type(
        doc(v![blockquote(v![p(v!["one<a>"]), p(v!["two<b>"])])]),
        doc(v![blockquote(v![h1(v!["one<a>"]), h1(v!["two<b>"])])]),
        "heading",
        Some(json!({"level": 1})),
    );
}

#[test]
fn setblocktype_clears_markup_when_necessary() {
    check_type(
        doc(v![p(v!["hello<a> ", em(v!["world"])])]),
        doc(v![pre(v!["hello world"])]),
        "code_block",
        None,
    );
}

#[test]
fn setblocktype_removes_non_allowed_nodes() {
    check_type(
        doc(v![p(v!["<a>one", img(), "two", img(), "three"])]),
        doc(v![pre(v!["onetwothree"])]),
        "code_block",
        None,
    );
}

#[test]
fn setblocktype_removes_newlines_in_non_code() {
    check_type(
        doc(v![pre(v!["<a>one\ntwo\nthree"])]),
        doc(v![p(v!["one two three"])]),
        "paragraph",
        None,
    );
}

#[test]
fn setblocktype_only_clears_markup_when_needed() {
    check_type(
        doc(v![p(v!["hello<a> ", em(v!["world"])])]),
        doc(v![h1(v!["hello<a> ", em(v!["world"])])]),
        "heading",
        Some(json!({"level": 1})),
    );
}

#[test]
fn setblocktype_skips_nodes_that_can_t_be_changed_due_to_constraints() {
    check_type(
        doc(v![
            p(v!["<a>hello", img()]),
            p(v!["okay"]),
            ul(v![li(v![p(v!["foo<b>"])])])
        ]),
        doc(v![
            pre(v!["<a>hello"]),
            pre(v!["okay"]),
            ul(v![li(v![p(v!["foo<b>"])])])
        ]),
        "code_block",
        None,
    );
}

#[test]
fn setnodemarkup_can_change_a_textblock() {
    check_markup(
        doc(v!["<a>", p(v!["foo"])]),
        doc(v![h1(v!["foo"])]),
        "heading",
        json!({"level": 1}),
    );
}

#[test]
fn setnodemarkup_can_change_an_inline_node() {
    check_markup(
        doc(v![p(v!["foo<a>", img(), "bar"])]),
        doc(v![p(v![
            "foo",
            n("image", json!({"src": "bar", "alt": "y"}), v![]),
            "bar"
        ])]),
        "image",
        json!({"src": "bar", "alt": "y"}),
    );
}

#[test]
fn replace_can_delete_text() {
    check_repl(
        doc(v![p(v!["hell<a>o y<b>ou"])]),
        None,
        doc(v![p(v!["hell<a><b>ou"])]),
    );
}

#[test]
fn replace_can_join_blocks() {
    check_repl(
        doc(v![p(v!["hell<a>o"]), p(v!["y<b>ou"])]),
        None,
        doc(v![p(v!["hell<a><b>ou"])]),
    );
}

#[test]
fn replace_can_delete_right_leaning_lopsided_regions() {
    check_repl(
        doc(v![blockquote(v![p(v!["ab<a>c"])]), "<b>", p(v!["def"])]),
        None,
        doc(v![blockquote(v![p(v!["ab<a>"])]), "<b>", p(v!["def"])]),
    );
}

#[test]
fn replace_can_delete_left_leaning_lopsided_regions() {
    check_repl(
        doc(v![p(v!["abc"]), "<a>", blockquote(v![p(v!["d<b>ef"])])]),
        None,
        doc(v![p(v!["abc"]), "<a>", blockquote(v![p(v!["<b>ef"])])]),
    );
}

#[test]
fn replace_can_overwrite_text() {
    check_repl(
        doc(v![p(v!["hell<a>o y<b>ou"])]),
        Some(doc(v![p(v!["<a>i k<b>"])])),
        doc(v![p(v!["hell<a>i k<b>ou"])]),
    );
}

#[test]
fn replace_can_insert_text() {
    check_repl(
        doc(v![p(v!["hell<a><b>o"])]),
        Some(doc(v![p(v!["<a>i k<b>"])])),
        doc(v![p(v!["helli k<a><b>o"])]),
    );
}

#[test]
fn replace_can_add_a_textblock() {
    check_repl(
        doc(v![p(v!["hello<a>you"])]),
        Some(doc(v!["<a>", p(v!["there"]), "<b>"])),
        doc(v![p(v!["hello"]), p(v!["there"]), p(v!["<a>you"])]),
    );
}

#[test]
fn replace_can_insert_while_joining_textblocks() {
    check_repl(
        doc(v![h1(v!["he<a>llo"]), p(v!["arg<b>!"])]),
        Some(doc(v![p(v!["1<a>2<b>3"])])),
        doc(v![h1(v!["he2!"])]),
    );
}

#[test]
fn replace_will_match_open_list_items() {
    check_repl(
        doc(v![ol(v![li(v![p(v!["one<a>"])]), li(v![p(v!["three"])])])]),
        Some(doc(v![ol(v![
            li(v![p(v!["<a>half"])]),
            li(v![p(v!["two"])]),
            "<b>"
        ])])),
        doc(v![ol(v![
            li(v![p(v!["onehalf"])]),
            li(v![p(v!["two"])]),
            li(v![p(v!["three"])])
        ])]),
    );
}

#[test]
fn replace_merges_blocks_across_deleted_content() {
    check_repl(
        doc(v![p(v!["a<a>"]), p(v!["b"]), p(v!["<b>c"])]),
        None,
        doc(v![p(v!["a<a><b>c"])]),
    );
}

#[test]
fn replace_can_merge_text_down_from_nested_nodes() {
    check_repl(
        doc(v![h1(v!["wo<a>ah"]), blockquote(v![p(v!["ah<b>ha"])])]),
        None,
        doc(v![h1(v!["wo<a><b>ha"])]),
    );
}

#[test]
fn replace_can_merge_text_up_into_nested_nodes() {
    check_repl(
        doc(v![
            blockquote(v![p(v!["foo<a>bar"])]),
            p(v!["middle"]),
            h1(v!["quux<b>baz"])
        ]),
        None,
        doc(v![blockquote(v![p(v!["foo<a><b>baz"])])]),
    );
}

#[test]
fn replace_will_join_multiple_levels_when_possible() {
    check_repl(
        doc(v![blockquote(v![ul(v![
            li(v![p(v!["a"])]),
            li(v![p(v!["b<a>"])]),
            li(v![p(v!["c"])]),
            li(v![p(v!["<b>d"])]),
            li(v![p(v!["e"])])
        ])])]),
        None,
        doc(v![blockquote(v![ul(v![
            li(v![p(v!["a"])]),
            li(v![p(v!["b<a><b>d"])]),
            li(v![p(v!["e"])])
        ])])]),
    );
}

#[test]
fn replace_can_replace_a_piece_of_text() {
    check_repl(
        doc(v![p(v!["he<before>llo<a> w<after>orld"])]),
        Some(doc(v![p(v!["<a> big<b>"])])),
        doc(v![p(v!["he<before>llo big w<after>orld"])]),
    );
}

#[test]
fn replace_respects_open_empty_nodes_at_the_edges() {
    check_repl(
        doc(v![p(v!["one<a>two"])]),
        Some(doc(v![p(v!["a<a>"]), p(v!["hello"]), p(v!["<b>b"])])),
        doc(v![p(v!["one"]), p(v!["hello"]), p(v!["<a>two"])]),
    );
}

#[test]
fn replace_can_completely_overwrite_a_paragraph() {
    check_repl(
        doc(v![
            p(v!["one<a>"]),
            p(v!["t<inside>wo"]),
            p(v!["<b>three<end>"])
        ]),
        Some(doc(v![p(v!["a<a>"]), p(v!["TWO"]), p(v!["<b>b"])])),
        doc(v![
            p(v!["one<a>"]),
            p(v!["TWO"]),
            p(v!["<inside>three<end>"])
        ]),
    );
}

#[test]
fn replace_joins_marks() {
    check_repl(
        doc(v![p(v!["foo ", em(v!["bar<a>baz"]), "<b> quux"])]),
        Some(doc(v![p(v!["foo ", em(v!["xy<a>zzy"]), " foo<b>"])])),
        doc(v![p(v!["foo ", em(v!["barzzy"]), " foo quux"])]),
    );
}

#[test]
fn replace_can_replace_text_with_a_break() {
    check_repl(
        doc(v![p(v!["foo<a>b<inside>b<b>bar"])]),
        Some(doc(v![p(v!["<a>", br(), "<b>"])])),
        doc(v![p(v!["foo", br(), "<inside>bar"])]),
    );
}

#[test]
fn replace_can_join_different_blocks() {
    check_repl(
        doc(v![h1(v!["hell<a>o"]), p(v!["by<b>e"])]),
        None,
        doc(v![h1(v!["helle"])]),
    );
}

#[test]
fn replace_can_restore_a_list_parent() {
    check_repl(
        doc(v![h1(v!["hell<a>o"]), "<b>"]),
        Some(doc(v![ol(v![
            li(v![p(v!["on<a>e"])]),
            li(v![p(v!["tw<b>o"])])
        ])])),
        doc(v![h1(v!["helle"]), ol(v![li(v![p(v!["tw"])])])]),
    );
}

#[test]
fn replace_can_restore_a_list_parent_and_join_text_after_it() {
    check_repl(
        doc(v![h1(v!["hell<a>o"]), p(v!["yo<b>u"])]),
        Some(doc(v![ol(v![
            li(v![p(v!["on<a>e"])]),
            li(v![p(v!["tw<b>o"])])
        ])])),
        doc(v![h1(v!["helle"]), ol(v![li(v![p(v!["twu"])])])]),
    );
}

#[test]
fn replace_can_insert_into_an_empty_block() {
    check_repl(
        doc(v![p(v!["a"]), p(v!["<a>"]), p(v!["b"])]),
        Some(doc(v![p(v!["x<a>y<b>z"])])),
        doc(v![p(v!["a"]), p(v!["y<a>"]), p(v!["b"])]),
    );
}

#[test]
fn replace_doesn_t_change_the_nesting_of_blocks_after_the_selection() {
    check_repl(
        doc(v![p(v!["one<a>"]), p(v!["two"]), p(v!["three"])]),
        Some(doc(v![
            p(v!["outside<a>"]),
            blockquote(v![p(v!["inside<b>"])])
        ])),
        doc(v![
            p(v!["one"]),
            blockquote(v![p(v!["inside"])]),
            p(v!["two"]),
            p(v!["three"])
        ]),
    );
}

#[test]
fn replace_can_close_a_parent_node() {
    check_repl(
        doc(v![blockquote(v![
            p(v!["b<a>c"]),
            p(v!["d<b>e"]),
            p(v!["f"])
        ])]),
        Some(doc(v![
            blockquote(v![p(v!["x<a>y"])]),
            p(v!["after"]),
            "<b>"
        ])),
        doc(v![
            blockquote(v![p(v!["b<a>y"])]),
            p(v!["after"]),
            blockquote(v![p(v!["<b>e"]), p(v!["f"])])
        ]),
    );
}

#[test]
fn replace_accepts_lopsided_regions() {
    check_repl(
        doc(v![blockquote(v![
            p(v!["b<a>c"]),
            p(v!["d<b>e"]),
            p(v!["f"])
        ])]),
        Some(doc(v![blockquote(v![p(v!["x<a>y"])]), p(v!["z<b>"])])),
        doc(v![
            blockquote(v![p(v!["b<a>y"])]),
            p(v!["z<b>e"]),
            blockquote(v![p(v!["f"])])
        ]),
    );
}

#[test]
fn replace_can_close_nested_parent_nodes() {
    check_repl(
        doc(v![blockquote(v![blockquote(v![
            p(v!["one"]),
            p(v!["tw<a>o"]),
            p(v!["t<b>hree<3>"]),
            p(v!["four<4>"])
        ])])]),
        Some(doc(v![
            ol(v![li(v![p(v!["hello<a>world"])]), li(v![p(v!["bye"])])]),
            p(v!["ne<b>xt"])
        ])),
        doc(v![blockquote(v![blockquote(v![
            p(v!["one"]),
            p(v!["tw<a>world"]),
            ol(v![li(v![p(v!["bye"])])]),
            p(v!["ne<b>hree<3>"]),
            p(v!["four<4>"])
        ])])]),
    );
}

#[test]
fn replace_will_close_open_nodes_to_the_right() {
    check_repl(
        doc(v![p(v!["x"]), "<a>"]),
        Some(doc(v![
            "<a>",
            ul(v![li(v![p(v!["a"])]), li(v!["<b>", p(v!["b"])])])
        ])),
        doc(v![
            p(v!["x"]),
            ul(v![li(v![p(v!["a"])]), li(v![p(v![])])]),
            "<a>"
        ]),
    );
}

#[test]
fn replace_can_delete_the_whole_document() {
    check_repl(
        doc(v!["<a>", h1(v!["hi"]), p(v!["you"]), "<b>"]),
        None,
        doc(v![p(v![])]),
    );
}

#[test]
fn replace_preserves_an_empty_parent_to_the_left() {
    check_repl(
        doc(v![blockquote(v!["<a>", p(v!["hi"])]), p(v!["b<b>x"])]),
        Some(doc(v![p(v!["<a>hi<b>"])])),
        doc(v![blockquote(v![p(v!["hix"])])]),
    );
}

#[test]
fn replace_drops_an_empty_parent_to_the_right() {
    check_repl(
        doc(v![
            p(v!["x<a>hi"]),
            blockquote(v![p(v!["yy"]), "<b>"]),
            p(v!["c"])
        ]),
        Some(doc(v![p(v!["<a>hi<b>"])])),
        doc(v![p(v!["xhi"]), p(v!["c"])]),
    );
}

#[test]
fn replace_drops_an_empty_node_at_the_start_of_the_slice() {
    check_repl(
        doc(v![p(v!["<a>x"])]),
        Some(doc(v![blockquote(v![p(v!["hi"]), "<a>"]), p(v!["b<b>"])])),
        doc(v![p(v![]), p(v!["bx"])]),
    );
}

#[test]
fn replace_drops_an_empty_node_at_the_end_of_the_slice() {
    check_repl(
        doc(v![p(v!["<a>x"])]),
        Some(doc(v![p(v!["b<a>"]), blockquote(v!["<b>", p(v!["hi"])])])),
        doc(v![p(v![]), blockquote(v![p(v![])]), p(v!["x"])]),
    );
}

#[test]
fn replace_doesn_t_drop_content_when_things_only_fit_at_the_top_level() {
    check_repl(
        doc(v![p(v!["foo"]), "<a>", p(v!["bar<b>"])]),
        Some(ol(v![li(v![p(v!["<a>a"])]), li(v![p(v!["b<b>"])])])),
        doc(v![p(v!["foo"]), p(v!["a"]), ol(v![li(v![p(v!["b"])])])]),
    );
}

#[test]
fn replace_will_auto_close_a_list_item_when_it_fits_in_a_list() {
    check_repl(
        doc(v![ul(v![
            li(v![p(v!["foo"])]),
            "<a>",
            li(v![p(v!["bar"])])
        ])]),
        Some(ul(v![li(v![p(v!["a<a>bc"])]), li(v![p(v!["de<b>f"])])])),
        doc(v![ul(v![
            li(v![p(v!["foo"])]),
            li(v![p(v!["bc"])]),
            li(v![p(v!["de"])]),
            li(v![p(v!["bar"])])
        ])]),
    );
}

#[test]
fn replacerange_replaces_inline_content() {
    check_repl_range(
        doc(v![p(v!["foo<a>b<b>ar"])]),
        p(v!["<a>xx<b>"]),
        doc(v![p(v!["foo<a>xx<b>ar"])]),
    );
}

#[test]
fn replacerange_replaces_an_empty_paragraph_with_a_heading() {
    check_repl_range(
        doc(v![p(v!["<a>"])]),
        doc(v![h1(v!["<a>text<b>"])]),
        doc(v![h1(v!["text"])]),
    );
}

#[test]
fn replacerange_replaces_a_fully_selected_paragraph_with_a_heading() {
    check_repl_range(
        doc(v![p(v!["<a>abc<b>"])]),
        doc(v![h1(v!["<a>text<b>"])]),
        doc(v![h1(v!["text"])]),
    );
}

#[test]
fn replacerange_recreates_a_list_when_overwriting_a_paragraph() {
    check_repl_range(
        doc(v![p(v!["<a>"])]),
        doc(v![ul(v![li(v![p(v!["<a>foobar<b>"])])])]),
        doc(v![ul(v![li(v![p(v!["foobar"])])])]),
    );
}

#[test]
fn replacerange_drops_context_when_it_doesn_t_fit() {
    check_repl_range(
        doc(v![ul(v![li(v![p(v!["<a>"])]), li(v![p(v!["b"])])])]),
        doc(v![h1(v!["<a>h<b>"])]),
        doc(v![ul(v![li(v![p(v!["h<a>"])]), li(v![p(v!["b"])])])]),
    );
}

#[test]
fn replacerange_can_replace_a_node_when_endpoints_are_in_different_children() {
    check_repl_range(
        doc(v![
            p(v!["a"]),
            ul(v![
                li(v![p(v!["<a>b"])]),
                li(v![p(v!["c"]), blockquote(v![p(v!["d<b>"])])])
            ]),
            p(v!["e"])
        ]),
        doc(v![h1(v!["<a>x<b>"])]),
        doc(v![p(v!["a"]), h1(v!["x"]), p(v!["e"])]),
    );
}

#[test]
fn replacerange_keeps_defining_context_when_inserting_at_the_start_of_a_textblock() {
    check_repl_range(
        doc(v![p(v!["<a>foo"])]),
        doc(v![ul(v![li(v![p(v!["<a>one"])]), li(v![p(v!["two<b>"])])])]),
        doc(v![ul(v![li(v![p(v!["one"])]), li(v![p(v!["twofoo"])])])]),
    );
}

#[test]
fn replacerange_drops_defining_context_when_it_matches_the_parent_structure() {
    check_repl_range(
        doc(v![blockquote(v![p(v!["<a>"])])]),
        doc(v![blockquote(v![p(v!["<a>one<b>"])])]),
        doc(v![blockquote(v![p(v!["one"])])]),
    );
}

#[test]
fn replacerange_drops_defining_context_when_it_matches_the_parent_structure_in_a_nested_context() {
    check_repl_range(
        doc(v![ul(v![li(v![
            p(v!["list1"]),
            blockquote(v![p(v!["<a>"])])
        ])])]),
        doc(v![blockquote(v![p(v!["<a>one<b>"])])]),
        doc(v![ul(v![li(v![
            p(v!["list1"]),
            blockquote(v![p(v!["one"])])
        ])])]),
    );
}

#[test]
fn replacerange_drops_defining_context_when_it_matches_the_parent_structure_in_a_deep_nested_context()
 {
    check_repl_range(
        doc(v![ul(v![li(v![
            p(v!["list1"]),
            ul(v![li(v![p(v!["list2"]), blockquote(v![p(v!["<a>"])])])])
        ])])]),
        doc(v![blockquote(v![p(v!["<a>one<b>"])])]),
        doc(v![ul(v![li(v![
            p(v!["list1"]),
            ul(v![li(v![p(v!["list2"]), blockquote(v![p(v!["one"])])])])
        ])])]),
    );
}

#[test]
fn replacerange_closes_open_nodes_at_the_start() {
    check_repl_range(
        doc(v!["<a>", p(v!["abc"]), "<b>"]),
        doc(v![ul(v![li(v!["<a>"])]), p(v!["def"]), "<b>"]),
        doc(v![ul(v![li(v![p(v![])])]), p(v!["def"])]),
    );
}

#[test]
fn replacerangewith_can_insert_an_inline_node() {
    check_repl_range_with(
        doc(v![p(v!["fo<a>o"])]),
        inline(img()),
        doc(v![p(v!["fo", img(), "<a>o"])]),
    );
}

#[test]
fn replacerangewith_can_replace_content_with_an_inline_node() {
    check_repl_range_with(
        doc(v![p(v!["<a>fo<b>o"])]),
        inline(img()),
        doc(v![p(v!["<a>", img(), "o"])]),
    );
}

#[test]
fn replacerangewith_can_replace_a_block_node_with_an_inline_node() {
    check_repl_range_with(
        doc(v!["<a>", blockquote(v![p(v!["a"])]), "<b>"]),
        inline(img()),
        doc(v![p(v![img()])]),
    );
}

#[test]
fn replacerangewith_can_replace_a_block_node_with_a_block_node() {
    check_repl_range_with(
        doc(v!["<a>", blockquote(v![p(v!["a"])]), "<b>"]),
        block(hr()),
        doc(v![hr()]),
    );
}

#[test]
fn replacerangewith_can_insert_a_block_quote_in_the_middle_of_text() {
    check_repl_range_with(
        doc(v![p(v!["foo<a>bar"])]),
        block(hr()),
        doc(v![p(v!["foo"]), hr(), p(v!["bar"])]),
    );
}

#[test]
fn replacerangewith_can_replace_empty_parents_with_a_block_node() {
    check_repl_range_with(
        doc(v![blockquote(v![p(v!["<a>"])])]),
        block(hr()),
        doc(v![blockquote(v![hr()])]),
    );
}

#[test]
fn replacerangewith_can_move_an_inserted_block_forward_out_of_parent_nodes() {
    check_repl_range_with(
        doc(v![h1(v!["foo<a>"])]),
        block(hr()),
        doc(v![h1(v!["foo"]), hr()]),
    );
}

#[test]
fn replacerangewith_can_move_an_inserted_block_backward_out_of_parent_nodes() {
    check_repl_range_with(
        doc(v![p(v!["a"]), blockquote(v![p(v!["<a>b"])])]),
        block(hr()),
        doc(v![p(v!["a"]), blockquote(v![hr(), p(v!["b"])])]),
    );
}

#[test]
fn deleterange_deletes_the_given_range() {
    check_delete_range(
        doc(v![p(v!["fo<a>o"]), p(v!["b<b>ar"])]),
        doc(v![p(v!["fo<a><b>ar"])]),
    );
}

#[test]
fn deleterange_deletes_empty_parent_nodes() {
    check_delete_range(
        doc(v![blockquote(v![
            ul(v![li(v!["<a>", p(v!["foo"]), "<b>"])]),
            p(v!["x"])
        ])]),
        doc(v![blockquote(v!["<a><b>", p(v!["x"])])]),
    );
}

#[test]
fn deleterange_doesn_t_delete_parent_nodes_that_can_be_empty() {
    check_delete_range(doc(v![p(v!["<a>foo<b>"])]), doc(v![p(v!["<a><b>"])]));
}

#[test]
fn deleterange_is_okay_with_deleting_empty_ranges() {
    check_delete_range(doc(v![p(v!["<a><b>"])]), doc(v![p(v!["<a><b>"])]));
}

#[test]
fn deleterange_will_delete_a_whole_covered_node_even_if_selection_ends_are_in_different_nodes() {
    check_delete_range(
        doc(v![
            ul(v![li(v![p(v!["<a>foo"])]), li(v![p(v!["bar<b>"])])]),
            p(v!["hi"])
        ]),
        doc(v![p(v!["hi"])]),
    );
}

#[test]
fn deleterange_leaves_wrapping_textblock_when_deleting_all_text_in_it() {
    check_delete_range(
        doc(v![p(v!["a"]), p(v!["<a>b<b>"])]),
        doc(v![p(v!["a"]), p(v![])]),
    );
}

#[test]
fn deleterange_expands_to_cover_the_whole_parent_node() {
    check_delete_range(
        doc(v![
            p(v!["a"]),
            blockquote(v![blockquote(v![p(v!["<a>foo"])]), p(v!["bar<b>"])]),
            p(v!["b"])
        ]),
        doc(v![p(v!["a"]), p(v!["b"])]),
    );
}

#[test]
fn deleterange_expands_to_cover_the_whole_document() {
    check_delete_range(
        doc(v![
            h1(v!["<a>foo"]),
            p(v!["bar"]),
            blockquote(v![p(v!["baz<b>"])])
        ]),
        doc(v![p(v![])]),
    );
}

#[test]
fn deleterange_doesn_t_expand_beyond_same_depth_textblocks() {
    check_delete_range(
        doc(v![h1(v!["<a>foo"]), p(v!["bar"]), p(v!["baz<b>"])]),
        doc(v![h1(v![])]),
    );
}

#[test]
fn deleterange_deletes_the_open_token_when_deleting_from_start_to_past_end_of_block() {
    check_delete_range(
        doc(v![h1(v!["<a>foo"]), p(v!["b<b>ar"])]),
        doc(v![p(v!["ar"])]),
    );
}

#[test]
fn deleterange_doesn_t_delete_the_open_token_when_the_range_end_is_at_end_of_its_own_block() {
    check_delete_range(
        doc(v![
            p(v!["one"]),
            h1(v!["<a>two"]),
            blockquote(v![p(v!["three<b>"])]),
            p(v!["four"])
        ]),
        doc(v![p(v!["one"]), h1(v![]), p(v!["four"])]),
    );
}

#[test]
fn deleterange_doesn_t_break_text_joining_by_inappropriate_expansion() {
    check_delete_range(
        doc(v![ol(v![li(v![
            p(v!["<a>One"]),
            ol(v![li(v![p(v!["Tw<b>o"])])])
        ])])]),
        doc(v![ol(v![li(v![p(v!["o"])])])]),
    );
}

#[test]
fn deleterange_will_delete_entire_blocks_when_deleting_from_the_start_of_one_textblock_to_another()
{
    check_delete_range(
        doc(v![
            blockquote(v![ol(v![
                li(v![p(v!["a"])]),
                li(v![p(v!["<a>b"])]),
                li(v![p(v!["c"])])
            ])]),
            p(v!["x"]),
            p(v!["<b>y"])
        ]),
        doc(v![blockquote(v![ol(v![li(v![p(v!["a"])])])]), p(v!["y"])]),
    );
}

#[test]
fn addnodemark_adds_a_mark() {
    check_add_node_mark(
        doc(v![p(v!["<a>", img()])]),
        mark("em"),
        doc(v![p(v!["<a>", em(v![img()])])]),
    );
}

#[test]
fn addnodemark_doesn_t_duplicate_a_mark() {
    check_add_node_mark(
        doc(v![p(v!["<a>", em(v![img()])])]),
        mark("em"),
        doc(v![p(v!["<a>", em(v![img()])])]),
    );
}

#[test]
fn addnodemark_replaces_a_mark() {
    check_add_node_mark(
        doc(v![p(v!["<a>", a(v![img()])])]),
        mark_with("link", json!({"href": "x"})),
        doc(v![p(v!["<a>", m("link", json!({"href": "x"}), v![img()])])]),
    );
}

#[test]
fn removenodemark_removes_a_mark() {
    check_rm_node_mark(
        doc(v![p(v!["<a>", em(v![img()])])]),
        MarkMatch::Mark(mark("em")),
        doc(v![p(v!["<a>", img()])]),
    );
}

#[test]
fn removenodemark_doesn_t_do_anything_when_there_is_no_mark() {
    check_rm_node_mark(
        doc(v![p(v!["<a>", img()])]),
        MarkMatch::Mark(mark("em")),
        doc(v![p(v!["<a>", img()])]),
    );
}

#[test]
fn removenodemark_can_remove_a_mark_from_multiple_marks() {
    check_rm_node_mark(
        doc(v![p(v!["<a>", em(v![a(v![img()])])])]),
        MarkMatch::Mark(mark("em")),
        doc(v![p(v!["<a>", a(v![img()])])]),
    );
}

#[test]
fn setnodeattribute_sets_an_attribute() {
    check_set_attr(
        doc(v!["<a>", h1(v!["a"])]),
        "level",
        json!(2),
        doc(v!["<a>", h2(v!["a"])]),
    );
}

// ---- hand-ported cases -----------------------------------------------------

fn comment_schema(doc_marks: Option<&str>) -> Schema {
    let mut doc_spec = NodeSpec::new().content("text*");
    if let Some(marks) = doc_marks {
        doc_spec = doc_spec.marks(marks);
    }
    Schema::new(
        SchemaSpec::default()
            .node("doc", doc_spec)
            .node("text", NodeSpec::new())
            .mark(
                "comment",
                MarkSpec::new()
                    .excludes("")
                    .attr("id", AttrSpec::required()),
            ),
    )
    .unwrap()
}

fn comment(schema: &Schema, id: i64) -> Mark {
    schema
        .mark_type("comment")
        .unwrap()
        .create(json!({"id": id}).as_object())
        .unwrap()
}

#[test]
fn addmark_does_not_remove_non_excluded_marks_of_the_same_type() {
    let s = comment_schema(None);
    let text = s
        .text("hi", MarkSet::from_marks([comment(&s, 10)]))
        .unwrap();
    let d = s
        .top_node_type()
        .create(None, Fragment::from_node(text), MarkSet::none())
        .unwrap();
    let mut tr = Transform::new(d);
    tr.add_mark(0, 2, &comment(&s, 20)).unwrap();
    assert_eq!(tr.doc.child(0).marks().len(), 2);
}

#[test]
fn addmark_can_remove_multiple_excluded_marks() {
    let s = Schema::new(
        SchemaSpec::default()
            .node("doc", NodeSpec::new().content("text*"))
            .node("text", NodeSpec::new())
            .mark("big", MarkSpec::new().excludes("small1 small2"))
            .mark("small1", MarkSpec::new())
            .mark("small2", MarkSpec::new()),
    )
    .unwrap();
    let mk = |name: &str| s.mark_type(name).unwrap().create(None).unwrap();
    let text = s
        .text("hi", MarkSet::from_marks([mk("small1"), mk("small2")]))
        .unwrap();
    let d = s
        .top_node_type()
        .create(None, Fragment::from_node(text), MarkSet::none())
        .unwrap();
    let mut tr = Transform::new(d);
    assert_eq!(tr.doc.child(0).marks().len(), 2);
    tr.add_mark(0, 2, &mk("big")).unwrap();
    assert_eq!(tr.doc.child(0).marks().len(), 1);
    assert_eq!(tr.doc.child(0).marks()[0].mark_type().name(), "big");
}

#[test]
fn removemark_can_remove_more_than_one_mark_of_the_same_type() {
    let s = comment_schema(None);
    let text = s
        .text("hi", MarkSet::from_marks([comment(&s, 1), comment(&s, 2)]))
        .unwrap();
    let d = s
        .top_node_type()
        .create(None, Fragment::from_node(text), MarkSet::none())
        .unwrap();
    let mut tr = Transform::new(d);
    assert_eq!(tr.doc.child(0).marks().len(), 2);
    tr.remove_mark(0, 2, &MarkMatch::Type(s.mark_type("comment").unwrap()))
        .unwrap();
    assert_eq!(tr.doc.child(0).marks().len(), 0);
}

/// The test schema with `hard_break` as the linebreak replacement.
fn lb_schema() -> Schema {
    let mut spec = schema_spec();
    for (name, node) in &mut spec.nodes {
        if name == "hard_break" {
            node.linebreak_replacement = true;
        }
    }
    Schema::new(spec).unwrap()
}

fn lb_doc(s: &Schema, children: Vec<Item>) -> TaggedNode {
    node_in(s, "doc", Value::Null, children)
}

fn lb_br() -> Item {
    n("hard_break", Value::Null, v![])
}

#[test]
fn join_converts_newlines_to_line_breaks() {
    let s = lb_schema();
    check_join(
        lb_doc(
            &s,
            v![
                n("paragraph", Value::Null, v!["one"]),
                "<a>",
                n("code_block", Value::Null, v!["two\nthree"])
            ],
        ),
        lb_doc(
            &s,
            v![n(
                "paragraph",
                Value::Null,
                v!["one<a>two", lb_br(), "three"]
            )],
        ),
    );
}

#[test]
fn join_converts_line_breaks_to_newlines() {
    let s = lb_schema();
    check_join(
        lb_doc(
            &s,
            v![
                n("code_block", Value::Null, v!["one"]),
                "<a>",
                n("paragraph", Value::Null, v!["two", lb_br(), "three"])
            ],
        ),
        lb_doc(&s, v![n("code_block", Value::Null, v!["one<a>two\nthree"])]),
    );
}

fn check_split(
    d: TaggedNode,
    expect: Option<TaggedNode>,
    depth: usize,
    types_after: Option<Vec<Option<NodeTypeWithAttrs>>>,
) {
    let mut tr = Transform::new(d.node.clone());
    let result = tr.split(d.tag("a"), depth, types_after.as_deref());
    match expect {
        Some(expect) => {
            result.unwrap();
            test_transform(&tr, &d, &expect);
        }
        None => assert!(result.is_err(), "split should fail"),
    }
}

#[test]
fn split_can_split_a_textblock() {
    check_split(
        doc(v![p(v!["foo<a>bar"])]),
        Some(doc(v![p(v!["foo"]), p(v!["<a>bar"])])),
        1,
        None,
    );
}

#[test]
fn split_correctly_maps_positions() {
    check_split(
        doc(v![p(v!["<1>a"]), p(v!["<2>foo<a>bar<3>"]), p(v!["<4>b"])]),
        Some(doc(v![
            p(v!["<1>a"]),
            p(v!["<2>foo"]),
            p(v!["<a>bar<3>"]),
            p(v!["<4>b"])
        ])),
        1,
        None,
    );
}

#[test]
fn split_can_split_two_deep() {
    check_split(
        doc(v![
            blockquote(v![blockquote(v![p(v!["foo<a>bar"])])]),
            p(v!["after<1>"])
        ]),
        Some(doc(v![
            blockquote(v![
                blockquote(v![p(v!["foo"])]),
                blockquote(v![p(v!["<a>bar"])])
            ]),
            p(v!["after<1>"])
        ])),
        2,
        None,
    );
}

#[test]
fn split_can_split_three_deep() {
    check_split(
        doc(v![
            blockquote(v![blockquote(v![p(v!["foo<a>bar"])])]),
            p(v!["after<1>"])
        ]),
        Some(doc(v![
            blockquote(v![blockquote(v![p(v!["foo"])])]),
            blockquote(v![blockquote(v![p(v!["<a>bar"])])]),
            p(v!["after<1>"])
        ])),
        3,
        None,
    );
}

#[test]
fn split_can_split_at_end() {
    check_split(
        doc(v![blockquote(v![p(v!["hi<a>"])])]),
        Some(doc(v![blockquote(v![p(v!["hi"]), p(v!["<a>"])])])),
        1,
        None,
    );
}

#[test]
fn split_can_split_at_start() {
    check_split(
        doc(v![blockquote(v![p(v!["<a>hi"])])]),
        Some(doc(v![blockquote(v![p(v![]), p(v!["<a>hi"])])])),
        1,
        None,
    );
}

#[test]
fn split_can_split_inside_a_list_item() {
    check_split(
        doc(v![ol(v![
            li(v![p(v!["one<1>"])]),
            li(v![p(v!["two<a>three"])]),
            li(v![p(v!["four<2>"])])
        ])]),
        Some(doc(v![ol(v![
            li(v![p(v!["one<1>"])]),
            li(v![p(v!["two"]), p(v!["<a>three"])]),
            li(v![p(v!["four<2>"])])
        ])])),
        1,
        None,
    );
}

#[test]
fn split_can_split_a_list_item() {
    check_split(
        doc(v![ol(v![
            li(v![p(v!["one<1>"])]),
            li(v![p(v!["two<a>three"])]),
            li(v![p(v!["four<2>"])])
        ])]),
        Some(doc(v![ol(v![
            li(v![p(v!["one<1>"])]),
            li(v![p(v!["two"])]),
            li(v![p(v!["<a>three"])]),
            li(v![p(v!["four<2>"])])
        ])])),
        2,
        None,
    );
}

#[test]
fn split_respects_the_type_param() {
    let paragraph = NodeTypeWithAttrs::new(schema().node_type("paragraph").unwrap());
    check_split(
        doc(v![h1(v!["hell<a>o!"])]),
        Some(doc(v![h1(v!["hell"]), p(v!["<a>o!"])])),
        1,
        Some(vec![Some(paragraph)]),
    );
}

#[test]
fn split_preserves_content_constraints_before() {
    check_split(doc(v![blockquote(v!["<a>", p(v!["x"])])]), None, 1, None);
}

#[test]
fn split_preserves_content_constraints_after() {
    check_split(doc(v![blockquote(v![p(v!["x"]), "<a>"])]), None, 1, None);
}

#[test]
fn settype_works_after_another_step() {
    let d = doc(v![p(v!["f<x>oob<y>ar"]), p(v!["baz<a>"])]);
    let mut tr = Transform::new(d.node.clone());
    tr.delete(d.tag("x"), d.tag("y")).unwrap();
    let pos = tr.mapping.map(d.tag("a"), Assoc::After);
    let heading = schema().node_type("heading").unwrap();
    tr.set_block_type(pos, pos, &heading, json!({"level": 1}).as_object())
        .unwrap();
    test_transform(&tr, &d, &doc(v![p(v!["f<x><y>ar"]), h1(v!["baz<a>"])]));
}

#[test]
fn settype_converts_newlines_to_linebreak_replacements() {
    let s = lb_schema();
    check_type(
        lb_doc(
            &s,
            v![n("code_block", Value::Null, v!["<a>one\ntwo\nthree"])],
        ),
        lb_doc(
            &s,
            v![n(
                "paragraph",
                Value::Null,
                v!["<a>one", lb_br(), "two", lb_br(), "three"]
            )],
        ),
        "paragraph",
        None,
    );
    check_type(
        lb_doc(&s, v![n("paragraph", Value::Null, v!["<a>one\ntwo"])]),
        lb_doc(&s, v![n("code_block", Value::Null, v!["<a>one\ntwo"])]),
        "code_block",
        None,
    );
}

#[test]
fn settype_converts_linebreak_replacements_to_newlines() {
    let s = lb_schema();
    check_type(
        lb_doc(
            &s,
            v![n(
                "paragraph",
                Value::Null,
                v!["<a>one", lb_br(), "two", lb_br(), "three"]
            )],
        ),
        lb_doc(
            &s,
            v![n("code_block", Value::Null, v!["<a>one\ntwo\nthree"])],
        ),
        "code_block",
        None,
    );
    check_type(
        lb_doc(
            &s,
            v![n(
                "paragraph",
                Value::Null,
                v!["<a>one", lb_br(), "two", lb_br(), "three"]
            )],
        ),
        lb_doc(
            &s,
            v![n(
                "heading",
                json!({"level": 1}),
                v!["<a>one", lb_br(), "two", lb_br(), "three"]
            )],
        ),
        "heading",
        Some(json!({"level": 1})),
    );
}

#[test]
fn settype_can_base_attributes_on_previous_attributes() {
    let d = doc(v!["<a>", h1(v!["a"]), p(v!["b"]), "<b>"]);
    let heading = schema().node_type("heading").unwrap();
    let mut tr = Transform::new(d.node.clone());
    tr.set_block_type_with(d.tag("a"), d.tag("b"), &heading, &|node| {
        let level = node.attr("level").and_then(Value::as_i64).unwrap_or(0);
        json!({"level": level + 1}).as_object().cloned()
    })
    .unwrap();
    test_transform(&tr, &d, &doc(v![h2(v!["a"]), h1(v!["b"])]));
}

#[test]
fn replace_does_nothing_when_given_an_unfittable_slice() {
    let content = Fragment::from_array(vec![
        node("blockquote", vec![]),
        node("horizontal_rule", vec![]),
    ]);
    check_repl_slice(p(v!["<a>x"]), Slice::new(content, 0, 0), p(v!["x"]));
}

#[test]
fn replace_preserves_open_end_when_top_isnt_placed() {
    let source = doc(v![ul(v![li(v![p(v!["ABCD"])]), li(v![p(v!["EFGH"])])])]);
    check_repl_slice(
        doc(v![ul(v![
            li(v![p(v!["ab<a>cd"])]),
            li(v![p(v!["ef<b>gh"])])
        ])]),
        source.node.slice(5, 13, true).unwrap(),
        doc(v![ul(v![li(v![p(v!["abCD"])]), li(v![p(v!["EFgh"])])])]),
    );
}

#[test]
fn replace_finds_the_proper_open_end_when_unwrapping_a_deep_slice() {
    let source = doc(v![blockquote(v![blockquote(v![blockquote(v![p(v![
        "hi"
    ])])])])]);
    check_repl_slice(
        doc(v!["<a>", p(v![]), "<b>"]),
        source.node.slice(3, 6, true).unwrap(),
        doc(v![p(v!["hi"])]),
    );
}

fn marks_schema() -> Schema {
    let mut spec = schema_spec();
    for (name, node) in &mut spec.nodes {
        if name == "doc" {
            node.marks = Some("_".into());
        }
    }
    Schema::new(spec).unwrap()
}

fn para(s: &Schema, text: &str, marks: &[&str]) -> Node {
    let marks = MarkSet::from_marks(
        marks
            .iter()
            .map(|m| s.mark_type(m).unwrap().create(None).unwrap()),
    );
    s.node_type("paragraph")
        .unwrap()
        .create(
            None,
            Fragment::from_node(s.text(text, MarkSet::none()).unwrap()),
            marks,
        )
        .unwrap()
}

fn doc_of(s: &Schema, children: Vec<Node>) -> Node {
    s.top_node_type()
        .create(None, Fragment::from_array(children), MarkSet::none())
        .unwrap()
}

#[test]
fn replace_preserves_marks_on_block_nodes() {
    let s = marks_schema();
    let mut tr = Transform::new(doc_of(
        &s,
        vec![para(&s, "hey", &["em"]), para(&s, "ok", &["strong"])],
    ));
    let slice = tr.doc.slice(2, 7, false).unwrap();
    tr.replace(2, 7, slice).unwrap();
    assert_eq!(tr.doc, *tr.before());
}

#[test]
fn replace_preserves_marks_on_open_slice_block_nodes() {
    let s = marks_schema();
    let mut tr = Transform::new(doc_of(&s, vec![para(&s, "a", &[])]));
    let source = doc_of(&s, vec![para(&s, "b", &["em"])]);
    tr.replace(3, 3, source.slice(1, 3, false).unwrap())
        .unwrap();
    assert_eq!(tr.doc.child_count(), 2);
    assert_eq!(tr.doc.last_child().unwrap().marks().len(), 1);
}

fn heading_body_schema() -> Schema {
    let mut spec = schema_spec();
    for (name, node) in &mut spec.nodes {
        if name == "doc" {
            node.content = Some("heading body".into());
        }
    }
    Schema::new(spec.node("body", NodeSpec::new().content("block+"))).unwrap()
}

fn hb_doc(s: &Schema, children: Vec<Item>) -> Node {
    node_in(s, "doc", Value::Null, children).node
}

fn hb_h(text: &str) -> Item {
    n(
        "heading",
        json!({"level": 1}),
        if text.is_empty() { v![] } else { v![text] },
    )
}

fn hb_p(text: &str) -> Item {
    n(
        "paragraph",
        Value::Null,
        if text.is_empty() { v![] } else { v![text] },
    )
}

fn hb_b(children: Vec<Item>) -> Item {
    n("body", Value::Null, children)
}

#[test]
fn replace_can_unwrap_a_paragraph_when_replacing_into_a_strict_schema() {
    let s = heading_body_schema();
    let mut tr = Transform::new(hb_doc(&s, v![hb_h("Head"), hb_b(v![hb_p("Content")])]));
    let (size, slice) = (tr.doc.content().size(), tr.doc.slice(7, 16, false).unwrap());
    tr.replace(0, size, slice).unwrap();
    assert_eq!(tr.doc, hb_doc(&s, v![hb_h("Content"), hb_b(v![hb_p("")])]));
}

#[test]
fn replace_can_unwrap_a_body_after_a_placed_node() {
    let s = heading_body_schema();
    let mut tr = Transform::new(hb_doc(&s, v![hb_h("Head"), hb_b(v![hb_p("Content")])]));
    let slice = tr.doc.slice(0, tr.doc.content().size(), false).unwrap();
    tr.replace(7, 7, slice).unwrap();
    assert_eq!(
        tr.doc,
        hb_doc(
            &s,
            v![
                hb_h("Head"),
                hb_b(v![hb_h("Head"), hb_p("Content"), hb_p("Content")])
            ]
        )
    );
}

#[test]
fn replace_can_wrap_a_paragraph_in_a_body_even_when_not_first() {
    let s = heading_body_schema();
    let mut tr = Transform::new(hb_doc(
        &s,
        v![hb_h("Head"), hb_b(v![hb_p("One"), hb_p("Two")])],
    ));
    let (size, slice) = (tr.doc.content().size(), tr.doc.slice(8, 16, false).unwrap());
    tr.replace(0, size, slice).unwrap();
    assert_eq!(tr.doc, hb_doc(&s, v![hb_h("One"), hb_b(v![hb_p("Two")])]));
}

#[test]
fn replace_can_split_a_fragment_and_place_children_in_different_parents() {
    let s = heading_body_schema();
    let mut tr = Transform::new(hb_doc(
        &s,
        v![hb_h("Head"), hb_b(v![hb_h("One"), hb_p("Two")])],
    ));
    let (size, slice) = (tr.doc.content().size(), tr.doc.slice(7, 17, false).unwrap());
    tr.replace(0, size, slice).unwrap();
    assert_eq!(tr.doc, hb_doc(&s, v![hb_h("One"), hb_b(v![hb_p("Two")])]));
}

#[test]
fn replace_will_insert_filler_nodes_before_a_node_when_necessary() {
    let s = heading_body_schema();
    let mut tr = Transform::new(hb_doc(&s, v![hb_h("Head"), hb_b(v![hb_p("One")])]));
    let size = tr.doc.content().size();
    let slice = tr.doc.slice(6, size, false).unwrap();
    tr.replace(0, size, slice).unwrap();
    assert_eq!(tr.doc, hb_doc(&s, v![hb_h(""), hb_b(v![hb_p("One")])]));
}

fn title_schema() -> Schema {
    let mut spec = schema_spec();
    for (name, node) in &mut spec.nodes {
        if name == "doc" {
            node.content = Some("title? block*".into());
        }
    }
    Schema::new(spec.node("title", NodeSpec::new().content("text*"))).unwrap()
}

#[test]
fn replace_does_not_fail_when_moving_text_would_solve_a_constraint() {
    let s = title_schema();
    let d = node_in(
        &s,
        "doc",
        Value::Null,
        v![n("title", Value::Null, v!["hi"])],
    )
    .node;
    let list = node_in(
        &s,
        "bullet_list",
        Value::Null,
        v![
            n(
                "list_item",
                Value::Null,
                v![n("paragraph", Value::Null, v!["one"])]
            ),
            n(
                "list_item",
                Value::Null,
                v![n("paragraph", Value::Null, v!["two"])]
            )
        ],
    )
    .node;
    let mut tr = Transform::new(d);
    tr.replace(1, 1, list.slice(2, 12, false).unwrap()).unwrap();
    assert!(!tr.steps.is_empty());
}

#[test]
fn replace_does_not_fail_pasting_title_and_code_block_into_an_empty_title() {
    let s = title_schema();
    let d = node_in(&s, "doc", Value::Null, v![n("title", Value::Null, v![])]).node;
    let source = node_in(
        &s,
        "doc",
        Value::Null,
        v![
            n("title", Value::Null, v!["title"]),
            n("code_block", Value::Null, v!["two"])
        ],
    )
    .node;
    let mut tr = Transform::new(d);
    tr.replace(
        1,
        1,
        source.slice(1, source.content().size(), false).unwrap(),
    )
    .unwrap();
    assert!(!tr.steps.is_empty());
}

#[test]
fn replace_does_not_fail_pasting_heading_and_code_block_into_an_empty_title() {
    let s = title_schema();
    let d = node_in(&s, "doc", Value::Null, v![n("title", Value::Null, v![])]).node;
    let source = node_in(
        &s,
        "doc",
        Value::Null,
        v![
            n("heading", json!({"level": 1}), v!["heading"]),
            n("code_block", Value::Null, v!["code"])
        ],
    )
    .node;
    let mut tr = Transform::new(d);
    tr.replace(
        1,
        1,
        source.slice(1, source.content().size(), false).unwrap(),
    )
    .unwrap();
    assert!(!tr.steps.is_empty());
}

#[test]
fn replace_can_handle_replacing_in_nodes_with_fixed_content() {
    let s = Schema::new(
        SchemaSpec::default()
            .node("doc", NodeSpec::new().content("block+"))
            .node("a", NodeSpec::new().content("inline*"))
            .node("b", NodeSpec::new().content("inline*"))
            .node("block", NodeSpec::new().content("a b"))
            .node("text", NodeSpec::new().group("inline")),
    )
    .unwrap();
    let d = node_in(
        &s,
        "doc",
        Value::Null,
        v![n(
            "block",
            Value::Null,
            v![n("a", Value::Null, v!["aa"]), n("b", Value::Null, v!["bb"])]
        )],
    )
    .node;
    let (from, to) = (3, d.content().size());
    let mut tr = Transform::new(d.clone());
    tr.replace(from, to, d.slice(from, to, false).unwrap())
        .unwrap();
    assert_eq!(tr.doc, d);
}

#[test]
fn replace_keeps_isolating_nodes_together() {
    let s = Schema::new(schema_spec().node(
        "iso",
        NodeSpec::new().group("block").content("block+").isolating(),
    ))
    .unwrap();
    let d = node_in(
        &s,
        "doc",
        Value::Null,
        v![n("paragraph", Value::Null, v!["one"])],
    )
    .node;
    let iso = node_in(
        &s,
        "doc",
        Value::Null,
        v![n(
            "iso",
            Value::Null,
            v![n("paragraph", Value::Null, v!["two"])]
        )],
    )
    .node
    .content()
    .clone();
    let mut tr = Transform::new(d.clone());
    tr.replace(2, 3, Slice::new(iso.clone(), 2, 0)).unwrap();
    let expected = node_in(
        &s,
        "doc",
        Value::Null,
        v![
            n("paragraph", Value::Null, v!["o"]),
            n(
                "iso",
                Value::Null,
                v![n("paragraph", Value::Null, v!["two"])]
            ),
            n("paragraph", Value::Null, v!["e"])
        ],
    )
    .node;
    assert_eq!(tr.doc, expected);
    let mut tr = Transform::new(d);
    tr.replace(2, 3, Slice::new(iso, 2, 2)).unwrap();
    assert_eq!(
        tr.doc,
        node_in(
            &s,
            "doc",
            Value::Null,
            v![n("paragraph", Value::Null, v!["otwoe"])]
        )
        .node
    );
}

#[test]
fn replace_range_keeps_defining_context_when_it_does_not_match_the_parent_markup() {
    let mut spec = schema_spec();
    for (name, node) in &mut spec.nodes {
        if name == "blockquote" {
            *node = NodeSpec::new()
                .content("block+")
                .group("block")
                .attr("color", AttrSpec::with_default("black"));
            node.defining_for_content = true;
        }
    }
    let s = Schema::new(spec).unwrap();
    let bq = |color: &str, children: Vec<Item>| n("blockquote", json!({"color": color}), children);
    let para = |text: &str| {
        n(
            "paragraph",
            Value::Null,
            if text.is_empty() { v![] } else { v![text] },
        )
    };
    let d = |children: Vec<Item>| node_in(&s, "doc", Value::Null, children);
    let source = || {
        d(v![
            bq("#100", v![para("<a>b1")]),
            bq("#200", v![para("b2<b>")])
        ])
    };

    let before1 = || vec![bq("#300", v![para("b3")]), bq("#400", v![para("<a>")])];
    let expect1 = || {
        vec![
            bq("#300", v![para("b3")]),
            bq("#100", v![para("b1")]),
            bq("#200", v![para("b2")]),
        ]
    };
    let mut before2 = vec![para("b5")];
    before2.extend(before1());
    let mut expect2 = vec![para("b5")];
    expect2.extend(expect1());
    let before3 = vec![para("b6"), bq("#500", before2.clone())];
    let expect3 = vec![para("b6"), bq("#500", expect2.clone())];

    check_repl_range(d(before1()), source(), d(expect1()));
    check_repl_range(
        d(vec![bq("#500", before2)]),
        source(),
        d(vec![bq("#500", expect2)]),
    );
    check_repl_range(
        d(vec![bq("#600", before3)]),
        source(),
        d(vec![bq("#600", expect3)]),
    );
}

#[test]
fn remove_node_mark_can_remove_multiple_instances_of_a_mark_type() {
    let s = Schema::new(
        SchemaSpec::default()
            .node("doc", NodeSpec::new().content("p+").marks("comment"))
            .node("p", NodeSpec::new().content("text*"))
            .node("text", NodeSpec::new())
            .mark(
                "comment",
                MarkSpec::new()
                    .excludes("")
                    .attr("id", AttrSpec::required()),
            ),
    )
    .unwrap();
    let para = s
        .node_type("p")
        .unwrap()
        .create(
            None,
            Fragment::from_node(s.text("abc", MarkSet::none()).unwrap()),
            MarkSet::from_marks([comment(&s, 1), comment(&s, 2)]),
        )
        .unwrap();
    let d = doc_of(&s, vec![para]);
    let mut tr = Transform::new(d.clone());
    tr.remove_node_mark(0, &MarkMatch::Type(s.mark_type("comment").unwrap()))
        .unwrap();
    let expect = node_in(&s, "doc", Value::Null, v![n("p", Value::Null, v!["abc"])]);
    let before = TaggedNode {
        node: d,
        tags: Default::default(),
    };
    test_transform(&tr, &before, &expect);
}

#[test]
fn set_doc_attribute_sets_an_attribute() {
    let s = Schema::new(
        SchemaSpec::default()
            .node(
                "doc",
                NodeSpec::new()
                    .content("text*")
                    .attr("meta", AttrSpec::with_default(Value::Null)),
            )
            .node("text", NodeSpec::new()),
    )
    .unwrap();
    let d = node_in(&s, "doc", Value::Null, v![]);
    let mut tr = Transform::new(d.node.clone());
    tr.set_doc_attribute("meta", json!("hello")).unwrap();
    test_transform(&tr, &d, &node_in(&s, "doc", json!({"meta": "hello"}), v![]));
}

fn changed(tr: &Transform) -> Option<String> {
    tr.changed_range().map(|(from, to)| format!("{from}-{to}"))
}

#[test]
fn changed_range_is_none_without_changes() {
    let mut tr = Transform::new(doc(v![p(v!["hello"])]).node);
    assert_eq!(changed(&tr), None);
    tr.add_mark(1, 3, &mark("strong")).unwrap();
    assert_eq!(changed(&tr), None);
}

#[test]
fn changed_range_covers_changes() {
    let mut tr = Transform::new(doc(v![p(v!["ab"])]).node);
    tr.insert(3, Fragment::from_node(text("c"))).unwrap();
    assert_eq!(changed(&tr).as_deref(), Some("3-4"));

    let mut tr = Transform::new(doc(v![p(v!["ab"])]).node);
    tr.insert(3, Fragment::from_node(text("c"))).unwrap();
    tr.insert(2, Fragment::from_node(text("d"))).unwrap();
    tr.insert(1, Fragment::from_node(text("e"))).unwrap();
    assert_eq!(changed(&tr).as_deref(), Some("1-6"));

    let mut tr = Transform::new(doc(v![p(v!["abcde"])]).node);
    tr.insert(6, Fragment::from_node(text("f"))).unwrap();
    tr.delete(1, 4).unwrap();
    assert_eq!(changed(&tr).as_deref(), Some("1-4"));
}

// ---- test-mapping.ts -------------------------------------------------------

fn mk(maps: &[[usize; 3]], mirrors: &[(usize, usize)]) -> Mapping {
    let mut mapping = Mapping::new();
    for map in maps {
        mapping.append_map(StepMap::new(map.to_vec()), None);
    }
    for (a, b) in mirrors {
        mapping.set_mirror(*a, *b);
    }
    mapping
}

fn test_mapping(mapping: &Mapping, cases: &[(usize, usize, Assoc, bool)]) {
    let inverted = mapping.invert();
    for (from, to, assoc, lossy) in cases {
        assert_eq!(mapping.map(*from, *assoc), *to, "map {from}");
        if !lossy {
            assert_eq!(inverted.map(*to, *assoc), *from, "unmap {to}");
        }
    }
}

const A: Assoc = Assoc::After;
const B: Assoc = Assoc::Before;

#[test]
fn mapping_through_insertions_deletions_and_replacements() {
    test_mapping(
        &mk(&[[2, 0, 4]], &[]),
        &[
            (0, 0, A, false),
            (2, 6, A, false),
            (2, 2, B, false),
            (3, 7, A, false),
        ],
    );
    test_mapping(
        &mk(&[[2, 4, 0]], &[]),
        &[
            (0, 0, A, false),
            (2, 2, B, false),
            (3, 2, A, true),
            (6, 2, A, false),
            (6, 2, B, true),
            (7, 3, A, false),
        ],
    );
    test_mapping(
        &mk(&[[2, 4, 4]], &[]),
        &[
            (0, 0, A, false),
            (2, 2, A, false),
            (4, 6, A, true),
            (4, 2, B, true),
            (6, 6, B, false),
            (8, 8, A, false),
        ],
    );
}

#[test]
fn mapping_through_mirrored_steps() {
    test_mapping(
        &mk(&[[2, 4, 0], [2, 0, 4]], &[(0, 1)]),
        &[
            (0, 0, A, false),
            (2, 2, A, false),
            (4, 4, A, false),
            (6, 6, A, false),
            (7, 7, A, false),
        ],
    );
    test_mapping(
        &mk(&[[2, 0, 4], [2, 4, 0]], &[(0, 1)]),
        &[(0, 0, A, false), (2, 2, A, false), (3, 3, A, false)],
    );
    test_mapping(
        &mk(&[[2, 4, 0], [1, 0, 1], [3, 0, 4]], &[(0, 2)]),
        &[
            (0, 0, A, false),
            (1, 2, A, false),
            (4, 5, A, false),
            (6, 7, A, false),
            (7, 8, A, false),
        ],
    );
}

fn del_flags(mapping: &Mapping, pos: usize, assoc: Assoc) -> String {
    let result = mapping.map_result(pos, assoc);
    let mut found = String::new();
    for (flag, set) in [
        ('d', result.deleted()),
        ('b', result.deleted_before()),
        ('a', result.deleted_after()),
        ('x', result.deleted_across()),
    ] {
        if set {
            found.push(flag);
        }
    }
    found
}

#[test]
fn mapping_assigns_deleted_flags() {
    let cases: &[(&[[usize; 3]], usize, Assoc, &str)] = &[
        (&[[0, 2, 0]], 2, B, "db"),
        (&[[0, 2, 0]], 2, A, "b"),
        (&[[0, 2, 2]], 2, B, "db"),
        (&[[0, 1, 0], [0, 1, 0]], 2, B, "db"),
        (&[[0, 1, 0]], 2, B, ""),
        (&[[2, 2, 0]], 2, B, "a"),
        (&[[2, 2, 0]], 2, A, "da"),
        (&[[2, 2, 2]], 2, A, "da"),
        (&[[2, 1, 0], [2, 1, 0]], 2, A, "da"),
        (&[[3, 2, 0]], 2, B, ""),
        (&[[0, 4, 0]], 2, B, "dbax"),
        (&[[0, 4, 0]], 2, A, "dbax"),
        (&[[0, 1, 0], [4, 1, 0], [0, 3, 0]], 2, A, "dbax"),
        (&[[4, 1, 0], [0, 1, 0]], 2, B, ""),
        (&[[2, 1, 0], [0, 2, 0]], 2, B, "dba"),
        (&[[2, 1, 0], [0, 1, 0]], 2, B, "a"),
        (&[[3, 1, 0], [0, 2, 0]], 2, B, "db"),
    ];
    for (maps, pos, assoc, flags) in cases {
        assert_eq!(
            del_flags(&mk(maps, &[]), *pos, *assoc),
            *flags,
            "{maps:?} at {pos}"
        );
    }
}

#[test]
fn lifting_a_non_first_list_item_child_stays_invertible() {
    // The gap inverse of this lift cannot apply (an empty list item must
    // start with a paragraph); the inverse falls back to a plain replace.
    let d = doc(v![ul(v![li(v![p(v!["i"]), "<a>", hr()])])]);
    let from = d.node.resolve(d.tag("a")).unwrap();
    let to = d.node.resolve(d.tag("a") + 1).unwrap();
    let range = from.block_range(&to, None).unwrap();
    let mut tr = Transform::new(d.node.clone());
    tr.lift(&range, lift_target(&range).unwrap()).unwrap();
    assert_eq!(tr.doc, doc(v![ul(v![li(v![p(v!["i"])])]), hr()]).node);
    assert_eq!(invert(&tr).doc, d.node);
}
