use rs_editor_theme::Theme;

use crate::*;

const ALL: &[Icon] = &[
    Icon::Bold,
    Icon::Italic,
    Icon::Underline,
    Icon::Strikethrough,
    Icon::Code,
    Icon::Highlighter,
    Icon::Link,
    Icon::Unlink,
    Icon::ExternalLink,
    Icon::Heading1,
    Icon::Heading2,
    Icon::Heading3,
    Icon::List,
    Icon::ListOrdered,
    Icon::ListTodo,
    Icon::Quote,
    Icon::SquareCode,
    Icon::Minus,
    Icon::Text,
    Icon::GripVertical,
    Icon::Image,
    Icon::Check,
    Icon::Copy,
    Icon::Pencil,
];

#[test]
fn every_icon_has_an_outline_inside_its_box() {
    for icon in ALL {
        let bounds = icon.path().bounds().to_owned();
        assert!(bounds.width() > 0.0 || bounds.height() > 0.0, "{icon:?}");
        // Control points may stick out a little.
        assert!(
            bounds.left >= -0.5
                && bounds.right <= 24.5
                && bounds.top >= -0.5
                && bounds.bottom <= 24.5,
            "{icon:?} {bounds:?}"
        );
    }
}

#[test]
fn toolbar_buttons_are_hit_where_they_are_drawn() {
    let mut toolbar = Toolbar::new(vec![
        ToolbarEntry::Button(ToolbarButton {
            icon: Icon::Bold,
            active: true,
            enabled: true,
        }),
        ToolbarEntry::Separator,
        ToolbarEntry::Button(ToolbarButton {
            icon: Icon::Italic,
            active: false,
            enabled: false,
        }),
        ToolbarEntry::Button(ToolbarButton {
            icon: Icon::Link,
            active: false,
            enabled: true,
        }),
    ]);
    toolbar.origin = (10.0, 20.0);
    let rects = toolbar.entry_rects();
    assert_eq!(
        toolbar.button_at(rects[0].center_x(), rects[0].center_y()),
        Some(0)
    );
    assert_eq!(
        toolbar.button_at(rects[1].center_x(), rects[1].center_y()),
        None,
        "separator"
    );
    assert_eq!(
        toolbar.button_at(rects[2].center_x(), rects[2].center_y()),
        None,
        "disabled"
    );
    assert_eq!(
        toolbar.button_at(rects[3].center_x(), rects[3].center_y()),
        Some(3)
    );
    assert!(toolbar.contains(12.0, 22.0));
    assert!(!toolbar.contains(5.0, 5.0));

    // A preview for eyeballing the icons: RS_EDITOR_UI_PREVIEW=out.png.
    if let Ok(path) = std::env::var("RS_EDITOR_UI_PREVIEW") {
        let mut surface = skia_safe::surfaces::raster_n32_premul((900, 120)).unwrap();
        for (row, theme) in [Theme::light(), Theme::dark()].iter().enumerate() {
            let canvas = surface.canvas();
            canvas.save();
            canvas.translate((0.0, row as f32 * 60.0));
            canvas.draw_rect(
                skia_safe::Rect::from_wh(900.0, 60.0),
                &skia_safe::Paint::new(
                    skia_safe::Color4f::from(skia_safe::Color::new(
                        theme.palette.background.argb(),
                    )),
                    None,
                ),
            );
            let mut bar = Toolbar::new(
                ALL.iter()
                    .enumerate()
                    .map(|(i, &icon)| {
                        ToolbarEntry::Button(ToolbarButton {
                            icon,
                            active: i == 0,
                            enabled: i != 5,
                        })
                    })
                    .collect(),
            );
            bar.origin = (8.0, 10.0);
            bar.hovered = Some(2);
            bar.paint(canvas, theme, 1.0);
            canvas.restore();
        }
        let image = surface.image_snapshot();
        let data = image
            .encode(None, skia_safe::EncodedImageFormat::PNG, None)
            .unwrap();
        std::fs::write(path, data.as_bytes()).unwrap();
    }
}

#[test]
fn text_field_edits_like_a_platform_field() {
    let mut field = TextField::new("rs-editor.dev");
    assert_eq!(field.selection(), (0, 13), "starts all selected");
    field.insert("https://x.y");
    assert_eq!(field.text(), "https://x.y");
    assert_eq!(field.selection(), (11, 11));

    field.delete(false, false);
    assert_eq!(field.text(), "https://x.");
    field.move_caret(false, true, false);
    assert_eq!(field.selection().1, 8, "word left stops at the host");
    field.move_to_edge(true, true);
    assert_eq!(field.selected_text(), "x.");
    field.insert("a\nb");
    assert_eq!(field.text(), "https://a b", "one line");

    // An emoji is one step, in UTF-16 units.
    let mut field = TextField::new("");
    field.insert("a👍b");
    field.move_caret(false, false, false);
    field.delete(false, false);
    assert_eq!(field.text(), "ab");
    assert_eq!(field.selection(), (1, 1));
}

#[test]
fn text_field_takes_ime_composition() {
    let mut field = TextField::new("");
    field.set_composing("に");
    field.set_composing("にほ");
    assert_eq!(field.composition(), Some((0, 2)));
    field.insert("日本");
    assert_eq!(field.text(), "日本");
    assert_eq!(field.composition(), None);

    // EditContext replaces ranges while composing.
    field.replace(2, 2, "ご", true);
    assert_eq!(field.composition(), Some((2, 3)));
    field.replace(2, 3, "語", false);
    assert_eq!(field.text(), "日本語");
    assert_eq!(field.composition(), None);

    field.set_selection(3, 3);
    field.delete_surrounding(1, 0);
    assert_eq!(field.text(), "日本");
    field.set_composing_region(0, 2);
    assert_eq!(field.composition(), Some((0, 2)));
    field.finish_composition();
    assert_eq!(field.composition(), None);
}

#[test]
fn menu_rows_scroll_and_are_hit_where_drawn() {
    let engine = rs_editor_layout::LayoutEngine::new(Theme::light(), false);
    let style = rs_editor_layout::UiText::small(Theme::light().palette.foreground);
    let mut menu = Menu {
        items: (0..12)
            .map(|i| MenuItem {
                icon: Icon::Text,
                label: engine.ui_label(&format!("Item {i}"), &style, None),
                hint: None,
            })
            .collect(),
        origin: (10.0, 20.0),
        max_height: 5.0 * ITEM_HEIGHT + 8.0,
        ..Menu::default()
    };
    assert_eq!(menu.size().1, 5.0 * ITEM_HEIGHT + 8.0);
    let row = menu.item_rect(2);
    assert_eq!(menu.item_at(row.center_x(), row.center_y()), Some(2));
    assert_eq!(menu.item_at(0.0, 0.0), None);

    menu.select_by(-1);
    assert_eq!(menu.selected, Some(11), "up from nothing wraps to the end");
    assert_eq!(menu.scroll, menu.max_scroll(), "revealed");
    let row = menu.item_rect(11);
    assert_eq!(menu.item_at(row.center_x(), row.center_y()), Some(11));
    menu.select_by(1);
    assert_eq!(menu.selected, Some(0));
    assert_eq!(menu.scroll, 0.0);
    assert!(menu.scroll_by(1_000.0));
    assert_eq!(menu.scroll, 7.0 * ITEM_HEIGHT);
}
