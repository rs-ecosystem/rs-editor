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
