//! Port of prosemirror-commands' `test-commands.ts`, plus list and keymap tests.

use std::sync::Arc;

use rs_editor_model::testing::*;
use rs_editor_model::{
    Fragment, MarkSet, MarkType, Node, NodeSpec, NodeType, Schema, SchemaSpec, v,
};
use rs_editor_state::testing::{TestState, sel_for};
use rs_editor_state::{Command, EditorState, EditorStateConfig, Selection, TextSelection, command};
use rs_editor_transform::NodeTypeWithAttrs;
use serde_json::{Value, json};

use crate::*;

fn nt(name: &str) -> NodeType {
    schema().node_type(name).unwrap()
}

fn mt(name: &str) -> MarkType {
    schema().mark_type(name).unwrap()
}

/// Runs `cmd` on `doc` (selection from its tags) and compares the result.
/// `None` means the command must leave the document unchanged.
fn check(d: TaggedNode, cmd: Command, result: Option<TaggedNode>) {
    let mut state = TestState::new(&d);
    state.command(|s, dispatch, view| cmd(s, dispatch, view));
    match result {
        Some(result) => {
            assert_eq!(
                state.doc(),
                &result.node,
                "\n  got  {}\n  want {}",
                state.doc(),
                result.node
            );
            if result.tags.contains_key("a") {
                assert_eq!(state.selection(), &sel_for(&result), "selection");
            }
        }
        None => assert_eq!(state.doc(), &d.node, "document should be unchanged"),
    }
}

/// Like [`check`], for documents of other schemas with an explicit cursor.
fn check_node(d: Node, cursor: usize, cmd: Command, result: Node) {
    let tagged = TaggedNode {
        node: d,
        tags: [("a".to_owned(), cursor)].into(),
    };
    check(
        tagged,
        cmd,
        Some(TaggedNode {
            node: result,
            tags: Default::default(),
        }),
    );
}

fn toggle(name: &str, options: ToggleMarkOptions) -> Command {
    toggle_mark(mt(name), None, options)
}

// ---- converted from test-commands.ts -----------------------------------------

#[test]
fn joinbackward_can_join_paragraphs() {
    check(
        doc(v![p(v!["hi"]), p(v!["<a>there"])]),
        command(join_backward),
        Some(doc(v![p(v!["hithere"])])),
    );
}

#[test]
fn joinbackward_can_join_out_of_a_nested_node() {
    check(
        doc(v![p(v!["hi"]), blockquote(v![p(v!["<a>there"])])]),
        command(join_backward),
        Some(doc(v![p(v!["hi"]), p(v!["there"])])),
    );
}

#[test]
fn joinbackward_moves_a_block_into_an_adjacent_wrapper() {
    check(
        doc(v![blockquote(v![p(v!["hi"])]), p(v!["<a>there"])]),
        command(join_backward),
        Some(doc(v![blockquote(v![p(v!["hi"]), p(v!["there"])])])),
    );
}

#[test]
fn joinbackward_moves_a_block_into_an_adjacent_wrapper_from_another_wrapper() {
    check(
        doc(v![
            blockquote(v![p(v!["hi"])]),
            blockquote(v![p(v!["<a>there"])])
        ]),
        command(join_backward),
        Some(doc(v![blockquote(v![p(v!["hi"]), p(v!["there"])])])),
    );
}

#[test]
fn joinbackward_joins_the_wrapper_to_a_subsequent_one_if_applicable() {
    check(
        doc(v![
            blockquote(v![p(v!["hi"])]),
            p(v!["<a>there"]),
            blockquote(v![p(v!["x"])])
        ]),
        command(join_backward),
        Some(doc(v![blockquote(v![
            p(v!["hi"]),
            p(v!["there"]),
            p(v!["x"])
        ])])),
    );
}

#[test]
fn joinbackward_moves_a_block_into_a_list_item() {
    check(
        doc(v![ul(v![li(v![p(v!["hi"])])]), p(v!["<a>there"])]),
        command(join_backward),
        Some(doc(v![ul(v![li(v![p(v!["hi"])]), li(v![p(v!["there"])])])])),
    );
}

#[test]
fn joinbackward_joins_lists() {
    check(
        doc(v![
            ul(v![li(v![p(v!["hi"])])]),
            ul(v![li(v![p(v!["<a>there"])])])
        ]),
        command(join_backward),
        Some(doc(v![ul(v![li(v![p(v!["hi"])]), li(v![p(v!["there"])])])])),
    );
}

#[test]
fn joinbackward_joins_list_items() {
    check(
        doc(v![ul(v![li(v![p(v!["hi"])]), li(v![p(v!["<a>there"])])])]),
        command(join_backward),
        Some(doc(v![ul(v![li(v![p(v!["hi"]), p(v!["there"])])])])),
    );
}

#[test]
fn joinbackward_lifts_out_of_a_list_at_the_start() {
    check(
        doc(v![ul(v![li(v![p(v!["<a>there"])])])]),
        command(join_backward),
        Some(doc(v![p(v!["<a>there"])])),
    );
}

#[test]
fn joinbackward_joins_lists_before_and_after() {
    check(
        doc(v![
            ul(v![li(v![p(v!["hi"])])]),
            p(v!["<a>there"]),
            ul(v![li(v![p(v!["x"])])])
        ]),
        command(join_backward),
        Some(doc(v![ul(v![
            li(v![p(v!["hi"])]),
            li(v![p(v!["there"])]),
            li(v![p(v!["x"])])
        ])])),
    );
}

#[test]
fn joinbackward_deletes_leaf_nodes_before() {
    check(
        doc(v![hr(), p(v!["<a>there"])]),
        command(join_backward),
        Some(doc(v![p(v!["there"])])),
    );
}

#[test]
fn joinbackward_lifts_before_it_deletes() {
    check(
        doc(v![hr(), blockquote(v![p(v!["<a>there"])])]),
        command(join_backward),
        Some(doc(v![hr(), p(v!["there"])])),
    );
}

#[test]
fn joinbackward_does_nothing_at_start_of_doc() {
    check(doc(v![p(v!["<a>foo"])]), command(join_backward), None);
}

#[test]
fn joinbackward_doesn_t_return_true_on_empty_blocks_that_can_t_be_deleted() {
    check(
        doc(v![
            p(v!["a"]),
            ul(v![li(v![p(v!["<a>"]), ul(v![li(v!["b"])])])])
        ]),
        command(join_backward),
        None,
    );
}

#[test]
fn joinbackward_doesn_t_join_surrounding_nodes_of_different_types() {
    check(
        doc(v![
            ul(v![li(v![p(v!["a"])])]),
            p(v!["<a>"]),
            ol(v![li(v![p(v!["b"])])])
        ]),
        command(join_backward),
        Some(doc(v![
            ul(v![li(v![p(v!["a"])]), li(v![p(v!["<a>"])])]),
            ol(v![li(v![p(v!["b"])])])
        ])),
    );
}

#[test]
fn jointextblockbackward_can_join_paragraphs() {
    check(
        doc(v![p(v!["hi"]), p(v!["<a>there"])]),
        command(join_textblock_backward),
        Some(doc(v![p(v!["hi<a>there"])])),
    );
}

#[test]
fn jointextblockbackward_can_join_if_second_block_is_wrapped() {
    check(
        doc(v![p(v!["hi"]), ul(v![li(v![p(v!["<a>there"])])])]),
        command(join_textblock_backward),
        Some(doc(v![p(v!["hi<a>there"])])),
    );
}

#[test]
fn jointextblockbackward_can_join_if_first_block_is_wrapped() {
    check(
        doc(v![blockquote(v![p(v!["hi"])]), p(v!["<a>there"])]),
        command(join_textblock_backward),
        Some(doc(v![blockquote(v![p(v!["hi<a>there"])])])),
    );
}

#[test]
fn jointextblockbackward_does_nothing_at_start_of_doc() {
    check(
        doc(v![p(v!["<a>foo"])]),
        command(join_textblock_backward),
        None,
    );
}

#[test]
fn jointextblockbackward_can_join_if_inside_a_nested_block() {
    check(
        doc(v![blockquote(v![
            blockquote(v![p(v!["hi"])]),
            p(v!["<a>there"])
        ])]),
        command(join_textblock_backward),
        Some(doc(v![blockquote(v![blockquote(v![p(v!["hi<a>there"])])])])),
    );
}

#[test]
fn selectnodebackward_selects_the_node_before_the_cut() {
    check(
        doc(v![
            blockquote(v![p(v!["a"])]),
            blockquote(v![p(v!["<a>b"])])
        ]),
        command(select_node_backward),
        Some(doc(v![
            "<a>",
            blockquote(v![p(v!["a"])]),
            blockquote(v![p(v!["b"])])
        ])),
    );
}

#[test]
fn selectnodebackward_does_nothing_when_not_at_the_start_of_the_textblock() {
    check(doc(v![p(v!["a<a>b"])]), command(select_node_backward), None);
}

#[test]
fn deleteselection_deletes_part_of_a_text_node() {
    check(
        doc(v![p(v!["f<a>o<b>o"])]),
        command(delete_selection),
        Some(doc(v![p(v!["fo"])])),
    );
}

#[test]
fn deleteselection_can_delete_across_blocks() {
    check(
        doc(v![p(v!["f<a>oo"]), p(v!["ba<b>r"])]),
        command(delete_selection),
        Some(doc(v![p(v!["fr"])])),
    );
}

#[test]
fn deleteselection_deletes_node_selections() {
    check(
        doc(v![p(v!["foo"]), "<a>", hr()]),
        command(delete_selection),
        Some(doc(v![p(v!["foo"])])),
    );
}

#[test]
fn deleteselection_moves_selection_after_deleted_node() {
    check(
        doc(v![
            p(v!["a"]),
            "<a>",
            p(v!["b"]),
            blockquote(v![p(v!["c"])])
        ]),
        command(delete_selection),
        Some(doc(v![p(v!["a"]), blockquote(v![p(v!["<a>c"])])])),
    );
}

#[test]
fn deleteselection_moves_selection_before_deleted_node_at_end() {
    check(
        doc(v![p(v!["a"]), "<a>", p(v!["b"])]),
        command(delete_selection),
        Some(doc(v![p(v!["a<a>"])])),
    );
}

#[test]
fn joinforward_joins_two_textblocks() {
    check(
        doc(v![p(v!["foo<a>"]), p(v!["bar"])]),
        command(join_forward),
        Some(doc(v![p(v!["foobar"])])),
    );
}

#[test]
fn joinforward_keeps_type_of_second_node_when_first_is_empty() {
    check(
        doc(v![p(v!["x"]), p(v!["<a>"]), h1(v!["hi"])]),
        command(join_forward),
        Some(doc(v![p(v!["x"]), h1(v!["<a>hi"])])),
    );
}

#[test]
fn joinforward_clears_nodes_from_joined_node_that_wouldn_t_be_allowed_in_target_node() {
    check(
        doc(v![pre(v!["foo<a>"]), p(v!["bar", img()])]),
        command(join_forward),
        Some(doc(v![pre(v!["foo<a>bar"])])),
    );
}

#[test]
fn joinforward_does_nothing_at_the_end_of_the_document() {
    check(doc(v![p(v!["foo<a>"])]), command(join_forward), None);
}

#[test]
fn joinforward_deletes_a_leaf_node_after_the_current_block() {
    check(
        doc(v![p(v!["foo<a>"]), hr(), p(v!["bar"])]),
        command(join_forward),
        Some(doc(v![p(v!["foo"]), p(v!["bar"])])),
    );
}

#[test]
fn joinforward_pulls_the_next_block_into_the_current_list_item() {
    check(
        doc(v![ul(v![li(v![p(v!["a<a>"])]), li(v![p(v!["b"])])])]),
        command(join_forward),
        Some(doc(v![ul(v![li(v![p(v!["a"]), p(v!["b"])])])])),
    );
}

#[test]
fn joinforward_joins_two_blocks_inside_of_a_list_item() {
    check(
        doc(v![ul(v![li(v![p(v!["a<a>"]), p(v!["b"])])])]),
        command(join_forward),
        Some(doc(v![ul(v![li(v![p(v!["ab"])])])])),
    );
}

#[test]
fn joinforward_pulls_the_next_block_into_a_blockquote() {
    check(
        doc(v![blockquote(v![p(v!["foo<a>"])]), p(v!["bar"])]),
        command(join_forward),
        Some(doc(v![blockquote(v![p(v!["foo<a>"]), p(v!["bar"])])])),
    );
}

#[test]
fn joinforward_joins_two_blockquotes() {
    check(
        doc(v![
            blockquote(v![p(v!["hi<a>"])]),
            blockquote(v![p(v!["there"])])
        ]),
        command(join_forward),
        Some(doc(v![blockquote(v![p(v!["hi"]), p(v!["there"])])])),
    );
}

#[test]
fn joinforward_pulls_the_next_block_outside_of_a_wrapping_blockquote() {
    check(
        doc(v![p(v!["foo<a>"]), blockquote(v![p(v!["bar"])])]),
        command(join_forward),
        Some(doc(v![p(v!["foo"]), p(v!["bar"])])),
    );
}

#[test]
fn joinforward_joins_two_lists() {
    check(
        doc(v![
            ul(v![li(v![p(v!["hi<a>"])])]),
            ul(v![li(v![p(v!["there"])])])
        ]),
        command(join_forward),
        Some(doc(v![ul(v![li(v![p(v!["hi"])]), li(v![p(v!["there"])])])])),
    );
}

#[test]
fn joinforward_does_nothing_in_a_nested_node_at_the_end_of_the_document() {
    check(
        doc(v![ul(v![li(v![p(v!["there<a>"])])])]),
        command(join_forward),
        None,
    );
}

#[test]
fn joinforward_deletes_a_leaf_node_at_the_end_of_the_document() {
    check(
        doc(v![p(v!["there<a>"]), hr()]),
        command(join_forward),
        Some(doc(v![p(v!["there"])])),
    );
}

#[test]
fn joinforward_moves_before_it_deletes_a_leaf_node() {
    check(
        doc(v![blockquote(v![p(v!["there<a>"])]), hr()]),
        command(join_forward),
        Some(doc(v![blockquote(v![p(v!["there"]), hr()])])),
    );
}

#[test]
fn joinforward_does_nothing_when_it_can_t_join() {
    check(
        doc(v![
            p(v!["foo<a>"]),
            ul(v![li(v![p(v!["bar"]), ul(v![li(v![p(v!["baz"])])])])])
        ]),
        command(join_forward),
        None,
    );
}

#[test]
fn jointextblockforward_can_join_paragraphs() {
    check(
        doc(v![p(v!["hi<a>"]), p(v!["there"])]),
        command(join_textblock_forward),
        Some(doc(v![p(v!["hi<a>there"])])),
    );
}

#[test]
fn jointextblockforward_can_join_if_second_block_is_wrapped() {
    check(
        doc(v![p(v!["hi<a>"]), ul(v![li(v![p(v!["there"])])])]),
        command(join_textblock_forward),
        Some(doc(v![p(v!["hi<a>there"])])),
    );
}

#[test]
fn jointextblockforward_can_join_if_first_block_is_wrapped() {
    check(
        doc(v![blockquote(v![p(v!["hi<a>"])]), p(v!["there"])]),
        command(join_textblock_forward),
        Some(doc(v![blockquote(v![p(v!["hi<a>there"])])])),
    );
}

#[test]
fn jointextblockforward_does_nothing_at_end_of_doc() {
    check(
        doc(v![p(v!["foo<a>"])]),
        command(join_textblock_forward),
        None,
    );
}

#[test]
fn selectnodeforward_selects_the_next_node() {
    check(
        doc(v![
            p(v!["foo<a>"]),
            ul(v![li(v![p(v!["bar"]), ul(v![li(v![p(v!["baz"])])])])])
        ]),
        command(select_node_forward),
        Some(doc(v![
            p(v!["foo<a>"]),
            "<a>",
            ul(v![li(v![p(v!["bar"]), ul(v![li(v![p(v!["baz"])])])])])
        ])),
    );
}

#[test]
fn selectnodeforward_does_nothing_at_end_of_document() {
    check(doc(v![p(v!["foo<a>"])]), command(select_node_forward), None);
}

#[test]
fn joinup_joins_identical_parent_blocks() {
    check(
        doc(v![
            blockquote(v![p(v!["foo"])]),
            blockquote(v![p(v!["<a>bar"])])
        ]),
        command(join_up),
        Some(doc(v![blockquote(v![p(v!["foo"]), p(v!["<a>bar"])])])),
    );
}

#[test]
fn joinup_does_nothing_in_the_first_block() {
    check(
        doc(v![
            blockquote(v![p(v!["<a>foo"])]),
            blockquote(v![p(v!["bar"])])
        ]),
        command(join_up),
        None,
    );
}

#[test]
fn joinup_joins_lists() {
    check(
        doc(v![
            ul(v![li(v![p(v!["foo"])])]),
            ul(v![li(v![p(v!["<a>bar"])])])
        ]),
        command(join_up),
        Some(doc(v![ul(v![li(v![p(v!["foo"])]), li(v![p(v!["bar"])])])])),
    );
}

#[test]
fn joinup_joins_list_items() {
    check(
        doc(v![ul(v![li(v![p(v!["foo"])]), li(v![p(v!["<a>bar"])])])]),
        command(join_up),
        Some(doc(v![ul(v![li(v![p(v!["foo"]), p(v!["bar"])])])])),
    );
}

#[test]
fn joinup_doesn_t_look_at_ancestors_when_a_block_is_selected() {
    check(
        doc(v![ul(v![
            li(v![p(v!["foo"])]),
            li(v!["<a>", p(v!["bar"])])
        ])]),
        command(join_up),
        None,
    );
}

#[test]
fn joinup_can_join_selected_block_nodes() {
    check(
        doc(v![ul(v![
            li(v![p(v!["foo"])]),
            "<a>",
            li(v![p(v!["bar"])])
        ])]),
        command(join_up),
        Some(doc(v![ul(v!["<a>", li(v![p(v!["foo"]), p(v!["bar"])])])])),
    );
}

#[test]
fn joindown_joins_parent_blocks() {
    check(
        doc(v![
            blockquote(v![p(v!["foo<a>"])]),
            blockquote(v![p(v!["bar"])])
        ]),
        command(join_down),
        Some(doc(v![blockquote(v![p(v!["foo<a>"]), p(v!["bar"])])])),
    );
}

#[test]
fn joindown_doesn_t_join_with_the_block_before() {
    check(
        doc(v![
            blockquote(v![p(v!["foo"])]),
            blockquote(v![p(v!["<a>bar"])])
        ]),
        command(join_down),
        None,
    );
}

#[test]
fn joindown_joins_lists() {
    check(
        doc(v![
            ul(v![li(v![p(v!["foo<a>"])])]),
            ul(v![li(v![p(v!["bar"])])])
        ]),
        command(join_down),
        Some(doc(v![ul(v![li(v![p(v!["foo"])]), li(v![p(v!["bar"])])])])),
    );
}

#[test]
fn joindown_joins_list_items() {
    check(
        doc(v![ul(v![li(v![p(v!["<a>foo"])]), li(v![p(v!["bar"])])])]),
        command(join_down),
        Some(doc(v![ul(v![li(v![p(v!["foo"]), p(v!["bar"])])])])),
    );
}

#[test]
fn joindown_doesn_t_look_at_parent_nodes_of_a_selected_node() {
    check(
        doc(v![ul(v![
            li(v!["<a>", p(v!["foo"])]),
            li(v![p(v!["bar"])])
        ])]),
        command(join_down),
        None,
    );
}

#[test]
fn joindown_can_join_selected_nodes() {
    check(
        doc(v![ul(v![
            "<a>",
            li(v![p(v!["foo"])]),
            li(v![p(v!["bar"])])
        ])]),
        command(join_down),
        Some(doc(v![ul(v!["<a>", li(v![p(v!["foo"]), p(v!["bar"])])])])),
    );
}

#[test]
fn lift_lifts_out_of_a_parent_block() {
    check(
        doc(v![blockquote(v![p(v!["<a>foo"])])]),
        command(lift),
        Some(doc(v![p(v!["<a>foo"])])),
    );
}

#[test]
fn lift_splits_the_parent_block_when_necessary() {
    check(
        doc(v![blockquote(v![
            p(v!["foo"]),
            p(v!["<a>bar"]),
            p(v!["baz"])
        ])]),
        command(lift),
        Some(doc(v![
            blockquote(v![p(v!["foo"])]),
            p(v!["bar"]),
            blockquote(v![p(v!["baz"])])
        ])),
    );
}

#[test]
fn lift_can_lift_out_of_a_list() {
    check(
        doc(v![ul(v![li(v![p(v!["<a>foo"])])])]),
        command(lift),
        Some(doc(v![p(v!["foo"])])),
    );
}

#[test]
fn lift_does_nothing_for_a_top_level_block() {
    check(doc(v![p(v!["<a>foo"])]), command(lift), None);
}

#[test]
fn lift_lifts_out_of_the_innermost_parent() {
    check(
        doc(v![blockquote(v![ul(v![li(v![p(v!["foo<a>"])])])])]),
        command(lift),
        Some(doc(v![blockquote(v![p(v!["foo<a>"])])])),
    );
}

#[test]
fn lift_can_lift_a_node_selection() {
    check(
        doc(v![blockquote(v!["<a>", ul(v![li(v![p(v!["foo"])])])])]),
        command(lift),
        Some(doc(v!["<a>", ul(v![li(v![p(v!["foo"])])])])),
    );
}

#[test]
fn lift_lifts_out_of_a_nested_list() {
    check(
        doc(v![ul(v![
            li(v![
                p(v!["one"]),
                ul(v![li(v![p(v!["<a>sub1"])]), li(v![p(v!["sub2"])])])
            ]),
            li(v![p(v!["two"])])
        ])]),
        command(lift),
        Some(doc(v![ul(v![
            li(v![
                p(v!["one"]),
                p(v!["<a>sub1"]),
                ul(v![li(v![p(v!["sub2"])])])
            ]),
            li(v![p(v!["two"])])
        ])])),
    );
}

#[test]
fn wrapin_can_wrap_a_paragraph() {
    check(
        doc(v![p(v!["fo<a>o"])]),
        wrap_in(nt("blockquote"), None),
        Some(doc(v![blockquote(v![p(v!["foo"])])])),
    );
}

#[test]
fn wrapin_wraps_multiple_pragraphs() {
    check(
        doc(v![
            p(v!["fo<a>o"]),
            p(v!["bar"]),
            p(v!["ba<b>z"]),
            p(v!["quux"])
        ]),
        wrap_in(nt("blockquote"), None),
        Some(doc(v![
            blockquote(v![p(v!["foo"]), p(v!["bar"]), p(v!["baz"])]),
            p(v!["quux"])
        ])),
    );
}

#[test]
fn wrapin_wraps_an_already_wrapped_node() {
    check(
        doc(v![blockquote(v![p(v!["fo<a>o"])])]),
        wrap_in(nt("blockquote"), None),
        Some(doc(v![blockquote(v![blockquote(v![p(v!["foo"])])])])),
    );
}

#[test]
fn wrapin_can_wrap_a_node_selection() {
    check(
        doc(v!["<a>", ul(v![li(v![p(v!["foo"])])])]),
        wrap_in(nt("blockquote"), None),
        Some(doc(v![blockquote(v![ul(v![li(v![p(v!["foo"])])])])])),
    );
}

#[test]
fn splitblock_splits_a_paragraph_at_the_end() {
    check(
        doc(v![p(v!["foo<a>"])]),
        command(split_block),
        Some(doc(v![p(v!["foo"]), p(v![])])),
    );
}

#[test]
fn splitblock_split_a_pragraph_in_the_middle() {
    check(
        doc(v![p(v!["foo<a>bar"])]),
        command(split_block),
        Some(doc(v![p(v!["foo"]), p(v!["bar"])])),
    );
}

#[test]
fn splitblock_splits_a_paragraph_from_a_heading() {
    check(
        doc(v![h1(v!["foo<a>"])]),
        command(split_block),
        Some(doc(v![h1(v!["foo"]), p(v![])])),
    );
}

#[test]
fn splitblock_splits_a_heading_in_two_when_in_the_middle() {
    check(
        doc(v![h1(v!["foo<a>bar"])]),
        command(split_block),
        Some(doc(v![h1(v!["foo"]), h1(v!["bar"])])),
    );
}

#[test]
fn splitblock_deletes_selected_content() {
    check(
        doc(v![p(v!["fo<a>ob<b>ar"])]),
        command(split_block),
        Some(doc(v![p(v!["fo"]), p(v!["ar"])])),
    );
}

#[test]
fn splitblock_splits_a_parent_block_when_a_node_is_selected() {
    check(
        doc(v![ol(v![
            li(v![p(v!["a"])]),
            "<a>",
            li(v![p(v!["b"])]),
            li(v![p(v!["c"])])
        ])]),
        command(split_block),
        Some(doc(v![
            ol(v![li(v![p(v!["a"])])]),
            ol(v![li(v![p(v!["b"])]), li(v![p(v!["c"])])])
        ])),
    );
}

#[test]
fn splitblock_doesn_t_split_the_parent_block_when_at_the_start() {
    check(
        doc(v![ol(v![
            "<a>",
            li(v![p(v!["a"])]),
            li(v![p(v!["b"])]),
            li(v![p(v!["c"])])
        ])]),
        command(split_block),
        None,
    );
}

#[test]
fn splitblock_splits_off_a_normal_paragraph_when_splitting_at_the_start_of_a_textblock() {
    check(
        doc(v![h1(v!["<a>foo"])]),
        command(split_block),
        Some(doc(v![p(v![]), h1(v!["foo"])])),
    );
}

#[test]
fn liftemptyblock_splits_the_parent_block_when_there_are_sibling_before() {
    check(
        doc(v![blockquote(v![p(v!["foo"]), p(v!["<a>"]), p(v!["bar"])])]),
        command(lift_empty_block),
        Some(doc(v![
            blockquote(v![p(v!["foo"])]),
            blockquote(v![p(v![]), p(v!["bar"])])
        ])),
    );
}

#[test]
fn liftemptyblock_lifts_the_last_child_out_of_its_parent() {
    check(
        doc(v![blockquote(v![p(v!["foo"]), p(v!["<a>"])])]),
        command(lift_empty_block),
        Some(doc(v![blockquote(v![p(v!["foo"])]), p(v![])])),
    );
}

#[test]
fn liftemptyblock_lifts_an_only_child() {
    check(
        doc(v![
            blockquote(v![p(v!["foo"])]),
            blockquote(v![p(v!["<a>"])])
        ]),
        command(lift_empty_block),
        Some(doc(v![blockquote(v![p(v!["foo"])]), p(v!["<a>"])])),
    );
}

#[test]
fn liftemptyblock_does_not_violate_schema_constraints() {
    check(
        doc(v![ul(v![li(v![
            p(v!["<a>foo"]),
            blockquote(v![p(v!["bar"])])
        ])])]),
        command(lift_empty_block),
        None,
    );
}

#[test]
fn liftemptyblock_lifts_out_of_a_list() {
    check(
        doc(v![ul(v![li(v![p(v!["hi"])]), li(v![p(v!["<a>"])])])]),
        command(lift_empty_block),
        Some(doc(v![ul(v![li(v![p(v!["hi"])])]), p(v![])])),
    );
}

#[test]
fn createparagraphnear_creates_a_paragraph_before_a_selected_node_at_the_start_of_the_doc() {
    check(
        doc(v!["<a>", hr(), hr()]),
        command(create_paragraph_near),
        Some(doc(v![p(v![]), hr(), hr()])),
    );
}

#[test]
fn createparagraphnear_creates_a_paragraph_after_a_lone_selected_node() {
    check(
        doc(v!["<a>", hr()]),
        command(create_paragraph_near),
        Some(doc(v![hr(), p(v![])])),
    );
}

#[test]
fn createparagraphnear_creates_a_paragraph_after_selected_nodes_not_at_the_start_of_the_doc() {
    check(
        doc(v![p(v![]), "<a>", hr()]),
        command(create_paragraph_near),
        Some(doc(v![p(v![]), hr(), p(v![])])),
    );
}

#[test]
fn setblocktype_can_change_the_type_of_a_paragraph() {
    check(
        doc(v![p(v!["fo<a>o"])]),
        set_block_type(nt("heading"), json!({"level": 1}).as_object().cloned()),
        Some(doc(v![h1(v!["foo"])])),
    );
}

#[test]
fn setblocktype_can_change_the_type_of_a_code_block() {
    check(
        doc(v![pre(v!["fo<a>o"])]),
        set_block_type(nt("heading"), json!({"level": 1}).as_object().cloned()),
        Some(doc(v![h1(v!["foo"])])),
    );
}

#[test]
fn setblocktype_can_make_a_heading_into_a_paragraph() {
    check(
        doc(v![h1(v!["fo<a>o"])]),
        set_block_type(nt("paragraph"), None),
        Some(doc(v![p(v!["foo"])])),
    );
}

#[test]
fn setblocktype_preserves_marks() {
    check(
        doc(v![h1(v!["fo<a>o", em(v!["bar"])])]),
        set_block_type(nt("paragraph"), None),
        Some(doc(v![p(v!["foo", em(v!["bar"])])])),
    );
}

#[test]
fn setblocktype_acts_on_node_selections() {
    check(
        doc(v!["<a>", h1(v!["foo"])]),
        set_block_type(nt("paragraph"), None),
        Some(doc(v![p(v!["foo"])])),
    );
}

#[test]
fn setblocktype_can_make_a_block_a_code_block() {
    check(
        doc(v![h1(v!["fo<a>o"])]),
        set_block_type(nt("code_block"), None),
        Some(doc(v![pre(v!["foo"])])),
    );
}

#[test]
fn setblocktype_clears_marks_when_necessary() {
    check(
        doc(v![p(v!["fo<a>o", em(v!["bar"])])]),
        set_block_type(nt("code_block"), None),
        Some(doc(v![pre(v!["foobar"])])),
    );
}

#[test]
fn setblocktype_acts_on_multiple_blocks_when_possible() {
    check(
        doc(v![
            p(v!["a<a>bc"]),
            p(v!["def"]),
            ul(v![li(v![p(v!["ghi"]), p(v!["jk<b>l"])])])
        ]),
        set_block_type(nt("code_block"), None),
        Some(doc(v![
            pre(v!["a<a>bc"]),
            pre(v!["def"]),
            ul(v![li(v![p(v!["ghi"]), pre(v!["jk<b>l"])])])
        ])),
    );
}

#[test]
fn setblocktype_returns_false_when_all_textblocks_in_the_selection_are_already_this_type() {
    check(
        doc(v![pre(v!["a<a>bc"]), pre(v!["de<b>f"])]),
        set_block_type(nt("code_block"), None),
        None,
    );
}

#[test]
fn setblocktype_returns_false_when_the_selected_blocks_can_t_be_changed() {
    check(
        doc(v![ul(v![p(v!["a<a>b<b>c"]), p(v!["def"])])]),
        set_block_type(nt("code_block"), None),
        None,
    );
}

#[test]
fn selectparentnode_selects_the_whole_textblock() {
    check(
        doc(v![ul(v![
            li(v![p(v!["foo"]), p(v!["b<a>ar"])]),
            li(v![p(v!["baz"])])
        ])]),
        command(select_parent_node),
        Some(doc(v![ul(v![
            li(v![p(v!["foo"]), "<a>", p(v!["bar"])]),
            li(v![p(v!["baz"])])
        ])])),
    );
}

#[test]
fn selectparentnode_goes_one_level_up_when_on_a_block() {
    check(
        doc(v![ul(v![
            li(v![p(v!["foo"]), "<a>", p(v!["bar"])]),
            li(v![p(v!["baz"])])
        ])]),
        command(select_parent_node),
        Some(doc(v![ul(v![
            "<a>",
            li(v![p(v!["foo"]), p(v!["bar"])]),
            li(v![p(v!["baz"])])
        ])])),
    );
}

#[test]
fn selectparentnode_goes_further_up() {
    check(
        doc(v![ul(v![
            "<a>",
            li(v![p(v!["foo"]), p(v!["bar"])]),
            li(v![p(v!["baz"])])
        ])]),
        command(select_parent_node),
        Some(doc(v![
            "<a>",
            ul(v![li(v![p(v!["foo"]), p(v!["bar"])]), li(v![p(v!["baz"])])])
        ])),
    );
}

#[test]
fn selectparentnode_stops_at_the_top_level() {
    check(
        doc(v![
            "<a>",
            ul(v![li(v![p(v!["foo"]), p(v!["bar"])]), li(v![p(v!["baz"])])])
        ]),
        command(select_parent_node),
        Some(doc(v![
            "<a>",
            ul(v![li(v![p(v!["foo"]), p(v!["bar"])]), li(v![p(v!["baz"])])])
        ])),
    );
}

// ---- hand-ported cases -------------------------------------------------------

#[test]
fn joinbackward_can_join_single_textblock_child_nodes() {
    let s = Schema::new(
        SchemaSpec::default()
            .node("text", NodeSpec::new().inline())
            .node("doc", NodeSpec::new().content("block+"))
            .node("block", NodeSpec::new().content("para"))
            .node("para", NodeSpec::new().content("text*")),
    )
    .unwrap();
    let d = node_in(
        &s,
        "doc",
        Value::Null,
        v![
            n("block", Value::Null, v![n("para", Value::Null, v!["a"])]),
            n("block", Value::Null, v![n("para", Value::Null, v!["b"])])
        ],
    )
    .node;
    let mut state = EditorState::create(EditorStateConfig {
        selection: Some(Selection::near(&d.resolve(7).unwrap(), 1)),
        doc: Some(d),
        ..Default::default()
    })
    .unwrap();
    let before = state.clone();
    assert!(join_backward(
        &before,
        Some(&mut |tr| state = before.apply(tr).unwrap()),
        None
    ));
    assert_eq!(state.doc().to_string(), r#"doc(block(para("ab")))"#);
}

/// The test schema with a heading-first document, an attribute-less
/// heading outside the block group, and an inline `span` with content.
fn heading_schema() -> Schema {
    let mut spec = schema_spec();
    for (name, node) in &mut spec.nodes {
        match name.as_str() {
            "heading" => *node = NodeSpec::new().content("inline*"),
            "doc" => node.content = Some("heading block*".into()),
            _ => {}
        }
    }
    Schema::new(spec.node(
        "span",
        NodeSpec::new().inline().group("inline").content("inline*"),
    ))
    .unwrap()
}

#[test]
fn splitblock_splits_a_paragraph_from_a_heading_when_a_double_heading_is_not_allowed() {
    let s = heading_schema();
    let d = node_in(
        &s,
        "doc",
        Value::Null,
        v![n("heading", Value::Null, v!["foobar"])],
    )
    .node;
    let result = node_in(
        &s,
        "doc",
        Value::Null,
        v![
            n("heading", Value::Null, v!["foo"]),
            n("paragraph", Value::Null, v!["bar"])
        ],
    )
    .node;
    check_node(d, 4, command(split_block), result);
}

#[test]
fn splitblock_does_not_reset_the_type_of_an_empty_leftover_when_forbidden() {
    let s = heading_schema();
    let d = node_in(
        &s,
        "doc",
        Value::Null,
        v![n("heading", Value::Null, v!["foobar"])],
    )
    .node;
    let result = node_in(
        &s,
        "doc",
        Value::Null,
        v![
            n("heading", Value::Null, v![]),
            n("paragraph", Value::Null, v!["foobar"])
        ],
    )
    .node;
    check_node(d, 1, command(split_block), result);
}

#[test]
fn splitblock_can_split_an_inline_node() {
    let s = heading_schema();
    let d = node_in(
        &s,
        "doc",
        Value::Null,
        v![n(
            "heading",
            Value::Null,
            v![n("span", Value::Null, v!["abcd"])]
        )],
    )
    .node;
    let result = node_in(
        &s,
        "doc",
        Value::Null,
        v![
            n("heading", Value::Null, v![n("span", Value::Null, v!["ab"])]),
            n(
                "paragraph",
                Value::Null,
                v![n("span", Value::Null, v!["cd"])]
            )
        ],
    )
    .node;
    check_node(d, 4, command(split_block), result);
}

#[test]
fn splitblock_prefers_textblocks() {
    let s = Schema::new(
        SchemaSpec::default()
            .node("text", NodeSpec::new())
            .node("para", NodeSpec::new().content("text*"))
            .node("section", NodeSpec::new().content("para+"))
            .node("doc", NodeSpec::new().content("para* section*")),
    )
    .unwrap();
    let d = node_in(
        &s,
        "doc",
        Value::Null,
        v![n("para", Value::Null, v!["hello"])],
    )
    .node;
    let result = node_in(
        &s,
        "doc",
        Value::Null,
        v![
            n("para", Value::Null, v!["he"]),
            n("para", Value::Null, v!["llo"])
        ],
    )
    .node;
    check_node(d, 3, command(split_block), result);
}

#[test]
fn split_block_as_splits_to_the_appropriate_type() {
    let split = split_block_as(Some(Arc::new(
        |node: &Node, _at_end: bool, _from: &rs_editor_model::ResolvedPos| {
            Some(NodeTypeWithAttrs {
                ty: node.node_type().schema().node_type("heading").unwrap(),
                attrs: json!({"level": 1}).as_object().cloned(),
            })
        },
    )));
    check(
        doc(v![p(v!["on<a>e"])]),
        split,
        Some(doc(v![p(v!["on"]), h1(v!["<a>e"])])),
    );
}

#[test]
fn split_block_as_passes_an_end_of_block_flag() {
    let split = split_block_as(Some(Arc::new(
        |node: &Node, at_end: bool, _from: &rs_editor_model::ResolvedPos| {
            at_end.then(|| {
                NodeTypeWithAttrs::new(node.node_type().schema().node_type("code_block").unwrap())
            })
        },
    )));
    check(
        doc(v![p(v!["one<a>"])]),
        split,
        Some(doc(v![p(v!["one"]), pre(v!["<a>"])])),
    );
}

#[test]
fn split_block_keep_marks_keeps_marks_after_marked_text() {
    let mut state = TestState::new(&doc(v![p(v![strong(v!["foo<a>"]), "bar"])]));
    state.command(split_block_keep_marks);
    assert_eq!(state.state.stored_marks().map(|m| m.len()), Some(1));
}

#[test]
fn split_block_keep_marks_preserves_the_stored_marks() {
    let mut state = TestState::new(&doc(v![p(v![em(v!["foo<a>"])])]));
    let strong = toggle("strong", ToggleMarkOptions::default());
    state.command(|s, d, v| strong(s, d, v));
    state.command(split_block_keep_marks);
    assert_eq!(state.state.stored_marks().map(|m| m.len()), Some(2));
}

fn bullet_lists_join() -> JoinPredicate {
    Arc::new(|before: &Node, _after: &Node| before.type_name() == "bullet_list")
}

#[test]
fn auto_join_joins_lists_when_deleting_a_paragraph_between_them() {
    check(
        doc(v![
            ul(v![li(v![p(v!["a"])])]),
            "<a>",
            p(v!["b"]),
            ul(v![li(v![p(v!["c"])])])
        ]),
        auto_join(command(delete_selection), bullet_lists_join()),
        Some(doc(v![ul(v![li(v![p(v!["a"])]), li(v![p(v!["c"])])])])),
    );
}

#[test]
fn auto_join_does_not_join_lists_when_deleting_an_item_inside_them() {
    check(
        doc(v![
            ul(v![li(v![p(v!["a"])]), "<a>", li(v![p(v!["b"])])]),
            ul(v![li(v![p(v!["c"])])])
        ]),
        auto_join(command(delete_selection), bullet_lists_join()),
        Some(doc(v![
            ul(v![li(v![p(v!["a"])])]),
            ul(v![li(v![p(v!["c"])])])
        ])),
    );
}

#[test]
fn auto_join_joins_lists_when_wrapping_a_paragraph_after_them() {
    check(
        doc(v![ul(v![li(v![p(v!["a"])])]), p(v!["b<a>"])]),
        auto_join(wrap_in(nt("bullet_list"), None), bullet_lists_join()),
        Some(doc(v![ul(v![li(v![p(v!["a"])]), li(v![p(v!["b"])])])])),
    );
}

#[test]
fn auto_join_joins_lists_when_wrapping_a_paragraph_between_them() {
    check(
        doc(v![
            ul(v![li(v![p(v!["a"])])]),
            p(v!["b<a>"]),
            ul(v![li(v![p(v!["c"])])])
        ]),
        auto_join(wrap_in(nt("bullet_list"), None), bullet_lists_join()),
        Some(doc(v![ul(v![
            li(v![p(v!["a"])]),
            li(v![p(v!["b"])]),
            li(v![p(v!["c"])])
        ])])),
    );
}

#[test]
fn auto_join_joins_lists_when_lifting_a_list_between_them() {
    check(
        doc(v![
            ul(v![li(v![p(v!["a"])])]),
            blockquote(v!["<a>", ul(v![li(v![p(v!["b"])])])]),
            ul(v![li(v![p(v!["c"])])])
        ]),
        auto_join(command(lift), bullet_lists_join()),
        Some(doc(v![ul(v![
            li(v![p(v!["a"])]),
            li(v![p(v!["b"])]),
            li(v![p(v!["c"])])
        ])])),
    );
}

fn em2() -> Command {
    toggle(
        "em",
        ToggleMarkOptions {
            remove_when_present: false,
            ..Default::default()
        },
    )
}

#[test]
fn toggle_mark_adds_stacks_and_removes_marks() {
    let em = || toggle("em", ToggleMarkOptions::default());
    check(
        doc(v![p(v!["one <a>two<b>"])]),
        em(),
        Some(doc(v![p(v!["one ", em_item(v!["two"])])])),
    );
    check(
        doc(v![p(v!["one <a>tw", strong(v!["o<b>"])])]),
        em(),
        Some(doc(v![p(v!["one ", em_item(v!["tw", strong(v!["o"])])])])),
    );
    check(
        doc(v![p(v![em_item(v!["one <a>two<b>"])])]),
        em(),
        Some(doc(v![p(v![em_item(v!["one "]), "two"])])),
    );
}

fn em_item(children: Vec<Item>) -> Item {
    rs_editor_model::testing::em(children)
}

#[test]
fn toggle_mark_toggles_pending_marks() {
    let mut state = TestState::new(&doc(v![p(v!["hell<a>o"])]));
    let em = toggle("em", ToggleMarkOptions::default());
    let strong = toggle("strong", ToggleMarkOptions::default());
    state.command(|s, d, v| em(s, d, v));
    assert_eq!(state.state.stored_marks().map(|m| m.len()), Some(1));
    state.command(|s, d, v| strong(s, d, v));
    assert_eq!(state.state.stored_marks().map(|m| m.len()), Some(2));
    state.command(|s, d, v| em(s, d, v));
    assert_eq!(state.state.stored_marks().map(|m| m.len()), Some(1));
}

#[test]
fn toggle_mark_handles_whitespace_at_selection_ends() {
    let em = || toggle("em", ToggleMarkOptions::default());
    check(
        doc(v![p(v!["one<a> two  <b>three"])]),
        em(),
        Some(doc(v![p(v!["one ", em_item(v!["two"]), "  three"])])),
    );
    check(
        doc(v![p(v!["one<a> <b>two"])]),
        em(),
        Some(doc(v![p(v!["one", em_item(v![" "]), "two"])])),
    );
    check(
        doc(v![p(v!["one<a> two  <b>three"])]),
        toggle(
            "em",
            ToggleMarkOptions {
                include_whitespace: true,
                ..Default::default()
            },
        ),
        Some(doc(v![p(v!["one", em_item(v![" two  "]), "three"])])),
    );
}

#[test]
fn toggle_mark_with_remove_when_present_off() {
    check(
        doc(v![p(v!["<a>", em_item(v!["one"]), " two<b>"])]),
        em2(),
        Some(doc(v![p(v![em_item(v!["one two"])])])),
    );
    check(
        doc(v![p(v!["<a>three<b>"])]),
        em2(),
        Some(doc(v![p(v![em_item(v!["three"])])])),
    );
    check(
        doc(v![p(v![em_item(v!["o<a>ne two<b>"])])]),
        em2(),
        Some(doc(v![p(v![em_item(v!["o"]), "ne two"])])),
    );
    check(
        doc(v![p(v![em_item(v!["o<a>ne two"]), "  <b>three"])]),
        em2(),
        Some(doc(v![p(v![em_item(v!["o"]), "ne two  three"])])),
    );
}

fn footnote_schema() -> Schema {
    Schema::new(
        SchemaSpec::default()
            .node("text", NodeSpec::new().inline())
            .node("doc", NodeSpec::new().content("para+"))
            .node("footnote", NodeSpec::new().content("text*").atom().inline())
            .node("para", NodeSpec::new().content("(text | footnote)*"))
            .mark("em", Default::default()),
    )
    .unwrap()
}

fn fdoc(s: &Schema, children: Vec<Item>) -> TaggedNode {
    node_in(s, "doc", Value::Null, children)
}

fn para(children: Vec<Item>) -> Item {
    n("para", Value::Null, children)
}

fn footnote(children: Vec<Item>) -> Item {
    n("footnote", Value::Null, children)
}

fn fem(children: Vec<Item>) -> Item {
    m("em", Value::Null, children)
}

fn atom_toggle(s: &Schema, enter: bool) -> Command {
    toggle_mark(
        s.mark_type("em").unwrap(),
        None,
        ToggleMarkOptions {
            enter_inline_atoms: enter,
            ..Default::default()
        },
    )
}

#[test]
fn toggle_mark_and_inline_atoms() {
    let s = footnote_schema();
    check(
        fdoc(&s, v![para(v!["h<a>ello", footnote(v!["okay"]), "<b>"])]),
        atom_toggle(&s, true),
        Some(fdoc(
            &s,
            v![para(v![
                "h",
                fem(v!["ello", footnote(v![fem(v!["okay"])])])
            ])],
        )),
    );
    check(
        fdoc(&s, v![para(v!["h<a>ello", footnote(v!["okay"]), "<b>"])]),
        atom_toggle(&s, false),
        Some(fdoc(
            &s,
            v![para(v!["h", fem(v!["ello", footnote(v!["okay"])])])],
        )),
    );
    check(
        fdoc(&s, v![para(v!["hello", footnote(v!["o<a>kay<b>"])])]),
        atom_toggle(&s, false),
        Some(fdoc(
            &s,
            v![para(v!["hello", footnote(v!["o", fem(v!["kay"])])])],
        )),
    );
    check(
        fdoc(
            &s,
            v![para(v!["h<a>ello", footnote(v![fem(v!["okay"])]), "<b>"])],
        ),
        atom_toggle(&s, false),
        Some(fdoc(
            &s,
            v![para(v![
                "h",
                fem(v!["ello", footnote(v![fem(v!["okay"])])])
            ])],
        )),
    );
    check(
        fdoc(
            &s,
            v![para(v![fem(v![
                "h<a>ello",
                footnote(v![fem(v!["okay"])]),
                "<b>"
            ])])],
        ),
        atom_toggle(&s, false),
        Some(fdoc(
            &s,
            v![para(v![
                fem(v!["h"]),
                "ello",
                footnote(v![fem(v!["okay"])])
            ])],
        )),
    );
}

#[test]
fn select_textblock_start_and_end() {
    let cases: Vec<(TaggedNode, CommandFnName, TaggedNode)> = vec![
        (
            doc(v![p(v!["one <a>two"])]),
            CommandFnName::Start,
            doc(v![p(v!["<a>one two"])]),
        ),
        (
            doc(v![p(v!["one <a>two"])]),
            CommandFnName::End,
            doc(v![p(v!["one two<a>"])]),
        ),
        (
            doc(v![p(v!["one <a>two<b>"])]),
            CommandFnName::Start,
            doc(v![p(v!["<a>one two"])]),
        ),
        (
            doc(v![p(v!["one <a>two<b>"])]),
            CommandFnName::End,
            doc(v![p(v!["one two<a>"])]),
        ),
        (
            doc(v![p(v!["one <a>two"]), p(v!["three<b> four"])]),
            CommandFnName::Start,
            doc(v![p(v!["<a>one two"]), p(v!["three four"])]),
        ),
        (
            doc(v![p(v!["one <a>two"]), p(v!["three<b> four"])]),
            CommandFnName::End,
            doc(v![p(v!["one two"]), p(v!["three four<a>"])]),
        ),
    ];
    for (start, which, expected) in cases {
        let cmd = match which {
            CommandFnName::Start => command(select_textblock_start),
            CommandFnName::End => command(select_textblock_end),
        };
        check(start, cmd, Some(expected));
    }
}

enum CommandFnName {
    Start,
    End,
}

// ---- lists ---------------------------------------------------------------------

fn wrap_ul() -> Command {
    wrap_in_list(nt("bullet_list"), None)
}

#[test]
fn lists_wrap_paragraphs_and_join_with_the_list_above() {
    check(
        doc(v![p(v!["<a>one"]), p(v!["two<b>"])]),
        wrap_ul(),
        Some(doc(v![ul(v![li(v![p(v!["one"])]), li(v![p(v!["two"])])])])),
    );
    check(
        doc(v![ul(v![li(v![p(v!["one"])]), li(v![p(v!["<a>two"])])])]),
        wrap_ul(),
        Some(doc(v![ul(v![li(v![
            p(v!["one"]),
            ul(v![li(v![p(v!["two"])])])
        ])])])),
    );
}

#[test]
fn lists_split_items_and_leave_the_list_from_an_empty_item() {
    let split = || split_list_item(nt("list_item"), None);
    check(
        doc(v![ul(v![li(v![p(v!["on<a>e"])])])]),
        split(),
        Some(doc(v![ul(v![li(v![p(v!["on"])]), li(v![p(v!["<a>e"])])])])),
    );
    // An empty last item is not split; Enter's chain then lifts it out.
    check(
        doc(v![ul(v![li(v![p(v!["one"])]), li(v![p(v!["<a>"])])])]),
        split(),
        None,
    );
    check(
        doc(v![ul(v![li(v![p(v!["one"])]), li(v![p(v!["<a>"])])])]),
        lift_list_item(nt("list_item")),
        Some(doc(v![ul(v![li(v![p(v!["one"])])]), p(v!["<a>"])])),
    );
}

#[test]
fn lists_split_an_empty_nested_item_into_the_parent_list() {
    check(
        doc(v![ul(v![li(v![
            p(v!["a"]),
            ul(v![li(v![p(v!["b"])]), li(v![p(v!["<a>"])])])
        ])])]),
        split_list_item(nt("list_item"), None),
        Some(doc(v![ul(v![
            li(v![p(v!["a"]), ul(v![li(v![p(v!["b"])])])]),
            li(v![p(v!["<a>"])])
        ])])),
    );
}

#[test]
fn lists_sink_and_lift_items() {
    let item = || nt("list_item");
    check(
        doc(v![ul(v![li(v![p(v!["one"])]), li(v![p(v!["t<a>wo"])])])]),
        sink_list_item(item()),
        Some(doc(v![ul(v![li(v![
            p(v!["one"]),
            ul(v![li(v![p(v!["t<a>wo"])])])
        ])])])),
    );
    check(
        doc(v![ul(v![li(v![p(v!["o<a>ne"])])])]),
        sink_list_item(item()),
        None,
    );
    check(
        doc(v![ul(v![li(v![
            p(v!["one"]),
            ul(v![li(v![p(v!["t<a>wo"])])])
        ])])]),
        lift_list_item(item()),
        Some(doc(v![ul(v![
            li(v![p(v!["one"])]),
            li(v![p(v!["t<a>wo"])])
        ])])),
    );
    check(
        doc(v![ul(v![li(v![p(v!["o<a>ne"])]), li(v![p(v!["two"])])])]),
        lift_list_item(item()),
        Some(doc(v![p(v!["o<a>ne"]), ul(v![li(v![p(v!["two"])])])])),
    );
}

// ---- keymap ----------------------------------------------------------------------

#[test]
fn key_names_normalize_per_platform() {
    assert_eq!(
        normalize_key_name("Mod-b", KeyPlatform::Apple).unwrap(),
        "Meta-b"
    );
    assert_eq!(
        normalize_key_name("Mod-b", KeyPlatform::Other).unwrap(),
        "Ctrl-b"
    );
    assert_eq!(
        normalize_key_name("Shift-Mod-z", KeyPlatform::Windows).unwrap(),
        "Shift-Ctrl-z"
    );
    assert_eq!(
        normalize_key_name("Alt-Ctrl-Space", KeyPlatform::Other).unwrap(),
        "Ctrl-Alt- "
    );
    assert_eq!(
        normalize_key_name("Mod--", KeyPlatform::Other).unwrap(),
        "Ctrl--"
    );
    assert!(normalize_key_name("Hyper-x", KeyPlatform::Other).is_err());
}

#[test]
fn keymap_runs_bindings_with_shift_and_layout_fallbacks() {
    let keymap = Keymap::new(
        vec![
            ("Mod-b", toggle("strong", ToggleMarkOptions::default())),
            ("Mod-a", command(select_all)),
        ],
        KeyPlatform::Other,
    )
    .unwrap();
    let mut state = TestState::new(&doc(v![p(v!["<a>one<b>"])]));
    let ctrl = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    let handled = state.command(|s, d, v| keymap.handle(&KeyEvent::new("b", ctrl), s, d, v));
    assert!(handled);
    assert_eq!(state.doc(), &doc(v![p(v![strong(v!["one"])])]).node);

    // A non-Latin layout produces another character; the physical key still binds.
    let event = KeyEvent {
        key: "ф".into(),
        code: Some("KeyA".into()),
        modifiers: ctrl,
    };
    assert!(state.command(|s, d, v| keymap.handle(&event, s, d, v)));
    assert!(state.selection().is_all());
    assert!(!state.command(|s, d, v| keymap.handle(&KeyEvent::new("x", ctrl), s, d, v)));
}

#[test]
fn base_keymap_enter_splits_and_backspace_joins() {
    let keymap = base_keymap(KeyPlatform::Apple);
    let mut state = TestState::new(&doc(v![p(v!["ab<a>cd"])]));
    assert!(state.command(|s, d, v| keymap.handle(
        &KeyEvent::new("Enter", Modifiers::default()),
        s,
        d,
        v
    )));
    assert_eq!(state.doc(), &doc(v![p(v!["ab"]), p(v!["cd"])]).node);
    assert!(state.command(|s, d, v| keymap.handle(
        &KeyEvent::new("Backspace", Modifiers::default()),
        s,
        d,
        v
    )));
    assert_eq!(state.doc(), &doc(v![p(v!["abcd"])]).node);
    let selection = state.selection().clone();
    assert!(matches!(selection, Selection::Text(_)));
    assert_eq!(selection.head(), 3);
    let _ = TextSelection::create(state.doc(), 1, 1).unwrap();
}

#[test]
fn commands_report_applicability_without_dispatching() {
    let state = TestState::new(&doc(v![p(v!["<a>one"])]));
    assert!(!delete_selection(&state.state, None, None));
    assert!(split_block(&state.state, None, None));
    assert_eq!(
        state.doc(),
        &doc(v![p(v!["one"])]).node,
        "dry runs never change the state"
    );
    let _ = (Fragment::empty(), MarkSet::none());
}
