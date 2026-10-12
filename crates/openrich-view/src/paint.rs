//! Drawing a frame: background, document, selection, composition
//! underline, caret and placeholder.

use openrich_extensions::starter_kit::Placeholder;
use openrich_layout::{Rect, render};
use openrich_state::Selection;
use skia_safe::{Canvas, Paint};

use crate::EditorView;

impl EditorView {
    /// Paints the view. `canvas` is in device pixels; the view applies its
    /// scale, padding and scroll offset.
    pub fn paint(&mut self, canvas: &Canvas) {
        let theme = self.theme().clone();
        let layout = self.layout().clone();
        let scale = self.scale;
        let background = skia_safe::Color::new(theme.palette.background.argb());
        canvas.clear(background);
        canvas.save();
        canvas.scale((scale, scale));
        canvas.translate((self.padding.left, self.padding.top - self.scroll_y));
        let viewport = Rect::new(
            -self.padding.left,
            self.scroll_y - self.padding.top,
            self.width,
            self.height,
        );

        render::paint_document(canvas, &layout, &theme, viewport);
        let selection = self.editor.selection().clone();
        match &selection {
            Selection::Node(node) => {
                if let Some(block) = layout
                    .blocks
                    .iter()
                    .find(|b| b.pos == selection.from() && b.node.ptr_eq(node.node()))
                {
                    render::paint_node_selection(canvas, block.rect, &theme);
                }
            }
            _ if !selection.empty() => {
                let rects = layout.selection_rects(selection.from(), selection.to());
                render::paint_selection(canvas, &rects, &theme);
            }
            _ => {}
        }

        self.paint_placeholder(canvas, &layout, &selection);

        if let Some((from, to)) = self.composition_range() {
            let mut underline = Paint::default();
            underline.set_anti_alias(true);
            underline.set_color(skia_safe::Color::new(theme.palette.foreground.argb()));
            for rect in layout.selection_rects(from, to) {
                let line = Rect::new(rect.x, rect.bottom() - 3.0, rect.width, 1.5);
                canvas.draw_rect(
                    skia_safe::Rect::from_xywh(line.x, line.y, line.width, line.height),
                    &underline,
                );
            }
        }

        if selection.empty()
            && matches!(selection, Selection::Text(_))
            && self.caret_visible()
            && let Some(rect) =
                layout.caret_rect(selection.head(), self.affinity, theme.spacing.caret_width)
        {
            render::paint_caret(canvas, rect, &theme);
        }
        canvas.restore();
    }

    fn paint_placeholder(
        &mut self,
        canvas: &Canvas,
        layout: &openrich_layout::DocumentLayout,
        selection: &Selection,
    ) {
        let Some(placeholder) = self
            .editor
            .manager()
            .extension::<Placeholder>("placeholder")
            .cloned()
        else {
            return;
        };
        let doc_empty = self.editor.is_empty();
        let cursor = selection.cursor().map(|c| c.pos());
        for text in &layout.texts {
            let current = cursor.is_some_and(|pos| pos == text.pos);
            let show = if placeholder.show_only_current {
                current || doc_empty
            } else {
                true
            };
            if !show || (doc_empty && text.pos != 1) {
                continue;
            }
            let Some(label) = placeholder.text_for(&text.node, doc_empty) else {
                continue;
            };
            let label_layout = self.engine.placeholder(&text.node, label, text.rect.width);
            render::paint_placeholder(canvas, &label_layout, text.rect.x, text.rect.y);
        }
    }
}
