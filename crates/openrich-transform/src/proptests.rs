//! Randomized invariants: every successful transform leaves a schema-valid
//! document, inverts back to the original and round-trips its steps as JSON.

use openrich_model::testing::schema;
use openrich_model::{Fragment, MarkSet, Node};
use proptest::prelude::*;

use crate::{MarkMatch, Step, Transform};

fn paragraph(text: &str, bold: bool) -> Node {
    let schema = schema();
    let marks = if bold {
        MarkSet::from_marks([schema.mark_type("strong").unwrap().create(None).unwrap()])
    } else {
        MarkSet::none()
    };
    let content = if text.is_empty() {
        Fragment::empty()
    } else {
        Fragment::from_node(schema.text(text, marks).unwrap())
    };
    schema
        .node_type("paragraph")
        .unwrap()
        .create(None, content, MarkSet::none())
        .unwrap()
}

fn block(kind: u8, text: &str, bold: bool) -> Node {
    let schema = schema();
    let para = paragraph(text, bold);
    let wrap = |name: &str, inner: Node| {
        schema
            .node_type(name)
            .unwrap()
            .create(None, Fragment::from_node(inner), MarkSet::none())
            .unwrap()
    };
    match kind % 4 {
        0 => para,
        1 => wrap("blockquote", para),
        2 => wrap("bullet_list", wrap("list_item", para)),
        _ => schema
            .node_type("heading")
            .unwrap()
            .create(None, para.content().clone(), MarkSet::none())
            .unwrap(),
    }
}

fn arb_doc() -> impl Strategy<Value = Node> {
    prop::collection::vec((any::<u8>(), "[ab🙂é ]{0,6}", any::<bool>()), 1..5).prop_map(|blocks| {
        let children = blocks
            .iter()
            .map(|(kind, text, bold)| block(*kind, text, *bold))
            .collect();
        schema()
            .top_node_type()
            .create(None, Fragment::from_array(children), MarkSet::none())
            .unwrap()
    })
}

#[derive(Debug, Clone)]
enum Op {
    Delete(usize, usize),
    Replace(usize, usize, usize, usize),
    AddMark(usize, usize),
    RemoveMark(usize, usize),
    Split(usize),
    Join(usize),
    DeleteRange(usize, usize),
}

fn arb_op() -> impl Strategy<Value = Op> {
    let p = || 0usize..200;
    prop_oneof![
        (p(), p()).prop_map(|(a, b)| Op::Delete(a, b)),
        (p(), p(), p(), p()).prop_map(|(a, b, c, d)| Op::Replace(a, b, c, d)),
        (p(), p()).prop_map(|(a, b)| Op::AddMark(a, b)),
        (p(), p()).prop_map(|(a, b)| Op::RemoveMark(a, b)),
        p().prop_map(Op::Split),
        p().prop_map(Op::Join),
        (p(), p()).prop_map(|(a, b)| Op::DeleteRange(a, b)),
    ]
}

fn range(doc: &Node, a: usize, b: usize) -> (usize, usize) {
    let size = doc.content().size() + 1;
    let (a, b) = (a % size, b % size);
    (a.min(b), a.max(b))
}

fn apply(tr: &mut Transform, op: &Op, source: &Node) {
    let doc = tr.doc.clone();
    let strong = schema().mark_type("strong").unwrap().create(None).unwrap();
    // Invalid positions (inside surrogate pairs) and impossible structures
    // are allowed to fail; only successful results are checked.
    let _ = match *op {
        Op::Delete(a, b) => {
            let (from, to) = range(&doc, a, b);
            tr.delete(from, to).map(|_| ())
        }
        Op::Replace(a, b, c, d) => {
            let (from, to) = range(&doc, a, b);
            let (sfrom, sto) = range(source, c, d);
            match source.slice(sfrom, sto, false) {
                Ok(slice) => tr.replace(from, to, slice).map(|_| ()),
                Err(_) => Ok(()),
            }
        }
        Op::AddMark(a, b) => {
            let (from, to) = range(&doc, a, b);
            tr.add_mark(from, to, &strong).map(|_| ())
        }
        Op::RemoveMark(a, b) => {
            let (from, to) = range(&doc, a, b);
            tr.remove_mark(from, to, &MarkMatch::All).map(|_| ())
        }
        Op::Split(a) => {
            let (pos, _) = range(&doc, a, a);
            if crate::can_split(&doc, pos, 1, None) {
                tr.split(pos, 1, None).map(|_| ())
            } else {
                Ok(())
            }
        }
        Op::Join(a) => {
            let (pos, _) = range(&doc, a, a);
            if crate::can_join(&doc, pos) {
                tr.join(pos, 1).map(|_| ())
            } else {
                Ok(())
            }
        }
        Op::DeleteRange(a, b) => {
            let (from, to) = range(&doc, a, b);
            tr.delete_range(from, to).map(|_| ())
        }
    };
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn transforms_stay_valid_invertible_and_serializable(
        doc in arb_doc(),
        source in arb_doc(),
        ops in prop::collection::vec(arb_op(), 1..10),
    ) {
        let mut tr = Transform::new(doc.clone());
        for op in &ops {
            apply(&mut tr, op, &source);
            prop_assert!(tr.doc.check().is_ok(), "invalid after {op:?}: {}", tr.doc);
        }
        let mut undo = Transform::new(tr.doc.clone());
        for i in (0..tr.steps.len()).rev() {
            let inverted = tr.steps[i].invert(&tr.docs[i]).unwrap();
            undo.step(inverted).unwrap();
        }
        prop_assert_eq!(&undo.doc, &doc);

        let mut replay = Transform::new(doc.clone());
        for step in &tr.steps {
            replay.step(Step::from_json(&schema(), &step.to_json()).unwrap()).unwrap();
        }
        prop_assert_eq!(&replay.doc, &tr.doc);

        // Every step's map must agree with the size change it causes.
        for (index, step) in tr.steps.iter().enumerate() {
            let before = tr.docs[index].content().size() as isize;
            let after = tr.docs.get(index + 1).unwrap_or(&tr.doc).content().size() as isize;
            let mut delta = 0isize;
            step.get_map().for_each(|old_start, old_end, new_start, new_end| {
                delta += (new_end - new_start) as isize - (old_end - old_start) as isize;
            });
            prop_assert_eq!(before + delta, after);
        }
    }
}
