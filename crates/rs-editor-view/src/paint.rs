//! Drawing a frame: background, document, selection, composition
//! underline, caret, placeholder, and the menus over them.

use rs_editor_extensions::starter_kit::Placeholder;
use rs_editor_layout::{Rect, render};
use rs_editor_state::Selection;
use skia_safe::{Canvas, Paint};

use crate::EditorView;

impl EditorView {
    /// Paints the view. `canvas` is in device pixels; the view applies its
    /// scale, padding and scroll offset.
    pub fn paint(&mut self, canvas: &Canvas) {
        self.tick_scroll();
        self.tick_touch();
        self.update_bubble();
        self.update_slash();
        self.update_link();
        self.tick_block_drag();
        self.update_block_handle();
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
                    // An image narrower than the column gets its own ring.
                    let image = layout.decorations.iter().find_map(|d| match d {
                        rs_editor_layout::Decoration::Image { rect, .. }
                            if block.rect.contains(rect.x + 1.0, rect.y + 1.0) =>
                        {
                            Some(*rect)
                        }
                        _ => None,
                    });
                    render::paint_node_selection(canvas, image.unwrap_or(block.rect), &theme);
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
            && !self.link_editing()
            && matches!(selection, Selection::Text(_))
            && self.caret_visible()
            && let Some(rect) =
                layout.caret_rect(selection.head(), self.affinity, theme.spacing.caret_width)
        {
            render::paint_caret(canvas, rect, &theme);
        }
        canvas.restore();
        // Handles and menus sit above everything, in view coordinates.
        canvas.save();
        canvas.scale((scale, scale));
        self.paint_handles(canvas);
        self.paint_block_handle(canvas);
        self.paint_bubble(canvas);
        self.paint_slash(canvas);
        self.paint_link(canvas);
        canvas.restore();
    }

    fn paint_placeholder(
        &mut self,
        canvas: &Canvas,
        layout: &rs_editor_layout::DocumentLayout,
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
            let label = match self.placeholder_override() {
                Some(custom) => custom,
                // The default text, in the UI's language: a block's name
                // for headings, a hint at the slash menu elsewhere.
                None if label == Placeholder::DEFAULT_TEXT => {
                    let level = text.node.attr("level").and_then(|v| v.as_u64());
                    match (text.node.type_name(), level) {
                        ("heading", Some(level @ 1..=3)) => {
                            self.strings.heading[level as usize - 1]
                        }
                        _ if text.node.node_type().spec().code => continue,
                        _ => self.strings.placeholder,
                    }
                }
                None => label,
            };
            let label_layout = self.engine.placeholder(&text.node, label, text.rect.width);
            render::paint_placeholder(canvas, &label_layout, text.rect.x, text.rect.y);
        }
    }
}

impl EditorView {
    /// Draws one of each kind of UI the editor shows (panels with their
    /// shadow, buttons, icons, menu rows, a text field, fading text) onto
    /// `canvas`. GPU shells call it once on a small offscreen surface
    /// soon after the first frame, so the GPU programs these need are
    /// compiled before a menu first opens (WebGL compiles them on first
    /// use, which would make that frame take hundreds of milliseconds).
    pub fn paint_ui_warm_up(&self, canvas: &Canvas) {
        use rs_editor_layout::UiText;
        use rs_editor_ui::{Icon, Menu, MenuItem, TextField, Toolbar, ToolbarButton, ToolbarEntry};

        let theme = self.theme();
        let palette = &theme.palette;
        let mut toolbar = Toolbar::new(
            [Icon::Bold, Icon::Link]
                .iter()
                .enumerate()
                .map(|(index, &icon)| {
                    ToolbarEntry::Button(ToolbarButton {
                        icon,
                        active: index == 0,
                        enabled: true,
                    })
                })
                .collect(),
        );
        toolbar.hovered = Some(1);
        toolbar.paint(canvas, theme, 1.0);
        toolbar.paint(canvas, theme, 0.5);

        let label = |text: &str, style: UiText| self.engine.ui_label(text, &style, Some(120.0));
        let menu = Menu {
            items: vec![MenuItem {
                icon: Icon::Text,
                label: label("Text", UiText::small(palette.popover_foreground)),
                hint: Some(label(
                    "#",
                    UiText {
                        mono: true,
                        ..UiText::extra_small(palette.muted_foreground)
                    },
                )),
            }],
            origin: (0.0, 48.0),
            width: 160.0,
            selected: Some(0),
            ..Menu::default()
        };
        menu.paint(canvas, theme, 1.0);
        menu.paint(canvas, theme, 0.5);

        let mut field = TextField::new("https://");
        let layout = label(field.text(), UiText::small(palette.foreground));
        let rect = skia_safe::Rect::from_xywh(8.0, 96.0, 160.0, 28.0);
        field.paint(canvas, theme, rect, &layout, None, true, true);
        field.move_to(2, false);
        field.invalid = true;
        field.paint(canvas, theme, rect, &layout, None, true, true);
    }
}
