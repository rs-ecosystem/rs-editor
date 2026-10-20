//! A list of commands on a panel, like shadcn's Command list: 32-pixel
//! rows with an icon, a label and an optional hint on the right, the
//! accent color behind the selected row, scrolling when taller than its
//! panel allows.

use rs_editor_layout::TextLayout;
use rs_editor_theme::Theme;
use skia_safe::{Canvas, Contains, Paint, Point, RRect, Rect};

use crate::{Icon, PANEL_PADDING, begin_appear, paint_panel, sk, with_alpha};

pub const ITEM_HEIGHT: f32 = 32.0;
const ICON_SIZE: f32 = 16.0;
const INSET: f32 = 8.0;

/// One row.
#[derive(Debug)]
pub struct MenuItem {
    pub icon: Icon,
    pub label: TextLayout,
    /// Shown muted at the right end (a Markdown shortcut, a key).
    pub hint: Option<TextLayout>,
}

/// A menu placed at `origin` (its top-left corner).
#[derive(Debug)]
pub struct Menu {
    pub items: Vec<MenuItem>,
    pub origin: (f32, f32),
    pub width: f32,
    /// The panel never grows past this height; rows scroll inside it.
    pub max_height: f32,
    /// The highlighted row (keyboard choice, or the row under the pointer).
    pub selected: Option<usize>,
    /// How far the rows are scrolled up.
    pub scroll: f32,
    /// Shown, muted, in place of rows when there are none ("No results").
    pub empty: Option<TextLayout>,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            origin: (0.0, 0.0),
            width: 240.0,
            max_height: 320.0,
            selected: None,
            scroll: 0.0,
            empty: None,
        }
    }
}

impl Menu {
    fn rows_height(&self) -> f32 {
        self.items.len().max(usize::from(self.empty.is_some())) as f32 * ITEM_HEIGHT
    }

    /// Width and height of the panel.
    pub fn size(&self) -> (f32, f32) {
        let inner = self
            .rows_height()
            .min((self.max_height - PANEL_PADDING * 2.0).max(ITEM_HEIGHT));
        (self.width, inner + PANEL_PADDING * 2.0)
    }

    pub fn rect(&self) -> Rect {
        let (width, height) = self.size();
        Rect::from_xywh(self.origin.0, self.origin.1, width, height)
    }

    /// The part of the panel the rows show through.
    fn viewport(&self) -> Rect {
        self.rect().with_inset((PANEL_PADDING, PANEL_PADDING))
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.rect().contains(Point::new(x, y))
    }

    /// Row `index`'s rectangle, where it is drawn (possibly scrolled out).
    pub fn item_rect(&self, index: usize) -> Rect {
        let viewport = self.viewport();
        Rect::from_xywh(
            viewport.left,
            viewport.top + index as f32 * ITEM_HEIGHT - self.scroll,
            viewport.width(),
            ITEM_HEIGHT,
        )
    }

    /// The row at `(x, y)`.
    pub fn item_at(&self, x: f32, y: f32) -> Option<usize> {
        let viewport = self.viewport();
        if !viewport.contains(Point::new(x, y)) {
            return None;
        }
        let index = ((y - viewport.top + self.scroll) / ITEM_HEIGHT).floor();
        (index >= 0.0 && (index as usize) < self.items.len()).then_some(index as usize)
    }

    /// Paints `text` in the first row's place, for a menu without rows.
    fn paint_empty(&self, canvas: &Canvas, text: &TextLayout, progress: f32) {
        let row = self.item_rect(0);
        paint_text(
            canvas,
            text,
            row.left + INSET,
            row.center_y() - text.height() / 2.0,
            progress,
        );
    }

    pub fn max_scroll(&self) -> f32 {
        (self.rows_height() - self.viewport().height()).max(0.0)
    }

    /// Scrolls the rows by `dy`; whether they moved.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll());
        self.scroll != before
    }

    /// Scrolls just enough to show row `index` whole.
    pub fn reveal(&mut self, index: usize) {
        let top = index as f32 * ITEM_HEIGHT;
        let visible = self.viewport().height();
        if top < self.scroll {
            self.scroll = top;
        } else if top + ITEM_HEIGHT > self.scroll + visible {
            self.scroll = top + ITEM_HEIGHT - visible;
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }

    /// Moves the selection `delta` rows, wrapping around the ends.
    pub fn select_by(&mut self, delta: i32) {
        let len = self.items.len() as i32;
        if len == 0 {
            self.selected = None;
            return;
        }
        let next = match self.selected {
            Some(current) => (current as i32 + delta).rem_euclid(len),
            None if delta < 0 => len - 1,
            None => 0,
        } as usize;
        self.selected = Some(next);
        self.reveal(next);
    }

    /// Paints the menu; `progress` (0 to 1) fades and zooms it in.
    pub fn paint(&self, canvas: &Canvas, theme: &Theme, progress: f32) {
        if progress <= 0.0 {
            return;
        }
        let rect = self.rect();
        begin_appear(canvas, rect, progress);
        paint_panel(canvas, rect, theme, progress);
        let palette = &theme.palette;
        let viewport = self.viewport();
        canvas.save();
        canvas.clip_rect(viewport, None, true);
        if self.items.is_empty()
            && let Some(empty) = &self.empty
        {
            self.paint_empty(canvas, empty, progress);
        }
        for (index, item) in self.items.iter().enumerate() {
            let row = self.item_rect(index);
            if row.bottom() < viewport.top || row.top > viewport.bottom() {
                continue;
            }
            let selected = self.selected == Some(index);
            if selected {
                let mut fill = Paint::default();
                fill.set_anti_alias(true);
                fill.set_color(with_alpha(sk(palette.accent), progress));
                canvas.draw_rrect(
                    RRect::new_rect_xy(row, theme.radius.sm, theme.radius.sm),
                    &fill,
                );
            }
            item.icon.draw(
                canvas,
                row.left + INSET,
                row.center_y() - ICON_SIZE / 2.0,
                ICON_SIZE,
                with_alpha(sk(palette.muted_foreground), progress),
            );
            let text_x = row.left + INSET + ICON_SIZE + INSET;
            paint_text(
                canvas,
                &item.label,
                text_x,
                row.center_y() - item.label.height() / 2.0,
                progress,
            );
            if let Some(hint) = &item.hint {
                paint_text(
                    canvas,
                    hint,
                    row.right - INSET - hint.width(),
                    row.center_y() - hint.height() / 2.0,
                    progress,
                );
            }
        }
        canvas.restore();
        // More rows above or below: the edge fades into the panel.
        let fade = |canvas: &Canvas, top: f32, down: bool| {
            // Strips from opaque at the edge to clear inward.
            const STRIPS: usize = 8;
            let height = ITEM_HEIGHT * 0.75;
            let strip = height / STRIPS as f32;
            for i in 0..STRIPS {
                let from_edge = if down { STRIPS - 1 - i } else { i };
                let opacity = 1.0 - (from_edge as f32 + 0.5) / STRIPS as f32;
                let mut paint = Paint::default();
                paint.set_color(with_alpha(sk(theme.palette.popover), progress * opacity));
                canvas.draw_rect(
                    Rect::from_xywh(
                        viewport.left,
                        top + i as f32 * strip,
                        viewport.width(),
                        strip,
                    ),
                    &paint,
                );
            }
        };
        if self.scroll > 0.5 {
            fade(canvas, viewport.top, false);
        }
        if self.scroll < self.max_scroll() - 0.5 {
            fade(canvas, viewport.bottom() - ITEM_HEIGHT * 0.75, true);
        }
        canvas.restore();
    }
}

/// Paints `text` (in its own color) at `opacity`.
pub fn paint_text(canvas: &Canvas, text: &TextLayout, x: f32, y: f32, opacity: f32) {
    if opacity >= 1.0 {
        text.paint(canvas, x, y);
        return;
    }
    let bounds = Rect::from_xywh(x - 2.0, y - 2.0, text.width() + 4.0, text.height() + 4.0);
    canvas.save_layer_alpha_f(bounds, opacity);
    text.paint(canvas, x, y);
    canvas.restore();
}
