//! Icons from Lucide (https://lucide.dev, ISC license), the set shadcn/ui
//! uses: 24 × 24 outlines stroked 2 units wide with round ends.

use skia_safe::{
    Canvas, Color, Paint, PaintCap, PaintJoin, PaintStyle, Path, PathBuilder, utils::parse_path,
};

/// An icon the editor's UI draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Code,
    Highlighter,
    Link,
    Unlink,
    ExternalLink,
    Heading1,
    Heading2,
    Heading3,
    List,
    ListOrdered,
    ListTodo,
    Quote,
    SquareCode,
    Minus,
    Text,
    GripVertical,
    Image,
    Check,
    Copy,
    Pencil,
}

impl Icon {
    /// SVG path data in Lucide's 24 × 24 box.
    fn paths(self) -> &'static [&'static str] {
        match self {
            Icon::Bold => {
                &["M6 12h9a4 4 0 0 1 0 8H7a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h7a4 4 0 0 1 0 8"]
            }
            Icon::Italic => &["M19 4h-9", "M14 20H5", "M15 4 9 20"],
            Icon::Underline => &["M6 4v6a6 6 0 0 0 12 0V4", "M4 20h16"],
            Icon::Strikethrough => &[
                "M16 4H9a3 3 0 0 0-2.83 4",
                "M14 12a4 4 0 0 1 0 8H6",
                "M4 12h16",
            ],
            Icon::Code => &["m16 18 6-6-6-6", "m8 6-6 6 6 6"],
            Icon::Highlighter => &[
                "m9 11-6 6v3h9l3-3",
                "m22 12-4.6 4.6a2 2 0 0 1-2.8 0l-5.2-5.2a2 2 0 0 1 0-2.8L14 4",
            ],
            Icon::Link => &[
                "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71",
                "M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71",
            ],
            Icon::Unlink => &[
                "m18.84 12.25 1.72-1.71h-.02a5.004 5.004 0 0 0-.12-7.07 5.006 5.006 0 0 0-6.95 0l-1.72 1.71",
                "m5.17 11.75-1.71 1.71a5.004 5.004 0 0 0 .12 7.07 5.006 5.006 0 0 0 6.95 0l1.71-1.71",
                "M8 2v3",
                "M2 8h3",
                "M16 22v-3",
                "M22 16h-3",
            ],
            Icon::ExternalLink => &[
                "M15 3h6v6",
                "M10 14 21 3",
                "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6",
            ],
            Icon::Heading1 => &["M4 12h8", "M4 18V6", "M12 18V6", "m17 12 3-2v8"],
            Icon::Heading2 => &[
                "M4 12h8",
                "M4 18V6",
                "M12 18V6",
                "M21 18h-4c0-4 4-3 4-6 0-1.5-2-2.5-4-1",
            ],
            Icon::Heading3 => &[
                "M4 12h8",
                "M4 18V6",
                "M12 18V6",
                "M17.5 10.5c1.7-1 3.5 0 3.5 1.5a2 2 0 0 1-2 2",
                "M17 17.5c2 1.5 4 .3 4-1.5a2 2 0 0 0-2-2",
            ],
            Icon::List => &[
                "M3 12h.01",
                "M3 18h.01",
                "M3 6h.01",
                "M8 12h13",
                "M8 18h13",
                "M8 6h13",
            ],
            Icon::ListOrdered => &[
                "M10 12h11",
                "M10 18h11",
                "M10 6h11",
                "M4 10h2",
                "M4 6h1v4",
                "M6 18H4c0-1 2-2 2-3s-1-1.5-2-1",
            ],
            Icon::ListTodo => &[
                "M3 5h6v6H3z",
                "m3 17 2 2 4-4",
                "M13 6h8",
                "M13 12h8",
                "M13 18h8",
            ],
            Icon::Quote => &[
                "M16 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2z",
                "M5 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2z",
            ],
            Icon::SquareCode => &[
                "m10 9-3 3 3 3",
                "m14 15 3-3-3-3",
                "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z",
            ],
            Icon::Minus => &["M5 12h14"],
            Icon::Text => &["M17 6.1H3", "M21 12.1H3", "M15.1 18H3"],
            Icon::GripVertical => &[
                "M9 12h.01",
                "M9 5h.01",
                "M9 19h.01",
                "M15 12h.01",
                "M15 5h.01",
                "M15 19h.01",
            ],
            Icon::Image => &[
                "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z",
                "M9 7a2 2 0 1 0 0 4 2 2 0 1 0 0-4z",
                "m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21",
            ],
            Icon::Check => &["M20 6 9 17l-5-5"],
            Icon::Copy => &[
                "M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2z",
                "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2",
            ],
            Icon::Pencil => &[
                "M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z",
                "m15 5 4 4",
            ],
        }
    }

    /// The icon's outline in Lucide's 24 × 24 box.
    pub fn path(self) -> Path {
        let mut path = PathBuilder::new();
        for data in self.paths() {
            if let Some(part) = parse_path::from_svg(data) {
                path.add_path(&part, None);
            }
        }
        path.detach()
    }

    /// Draws the icon `size` pixels wide with its top-left at `(x, y)`.
    pub fn draw(self, canvas: &Canvas, x: f32, y: f32, size: f32, color: Color) {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(2.0);
        paint.set_stroke_cap(PaintCap::Round);
        paint.set_stroke_join(PaintJoin::Round);
        paint.set_color(color);
        canvas.save();
        canvas.translate((x, y));
        canvas.scale((size / 24.0, size / 24.0));
        canvas.draw_path(&self.path(), &paint);
        canvas.restore();
    }
}
