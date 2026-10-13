//! A row of icon buttons on a panel, like shadcn's toggle group in a
//! popover: 32-pixel buttons, the accent color behind active ones, muted
//! hover, half opacity when disabled.

use rs_editor_theme::Theme;
use skia_safe::{Canvas, Contains, Paint, RRect, Rect};

use crate::{Icon, PANEL_PADDING, paint_panel, sk, with_alpha};

pub const BUTTON_SIZE: f32 = 32.0;
const ICON_SIZE: f32 = 16.0;
const GAP: f32 = 2.0;
const SEPARATOR: f32 = 9.0;

/// One button.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolbarButton {
    pub icon: Icon,
    /// Shown pressed in (the mark or block is active at the selection).
    pub active: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolbarEntry {
    Button(ToolbarButton),
    Separator,
}

/// A toolbar placed at `origin` (its top-left corner).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Toolbar {
    pub entries: Vec<ToolbarEntry>,
    pub origin: (f32, f32),
    /// Entry under the pointer.
    pub hovered: Option<usize>,
    /// Entry held down.
    pub pressed: Option<usize>,
}

impl Toolbar {
    pub fn new(entries: Vec<ToolbarEntry>) -> Self {
        Self {
            entries,
            ..Self::default()
        }
    }

    /// Width and height of the panel.
    pub fn size(&self) -> (f32, f32) {
        let mut width = PANEL_PADDING * 2.0;
        for (index, entry) in self.entries.iter().enumerate() {
            if index > 0 {
                width += GAP;
            }
            width += match entry {
                ToolbarEntry::Button(_) => BUTTON_SIZE,
                ToolbarEntry::Separator => SEPARATOR,
            };
        }
        (width, BUTTON_SIZE + PANEL_PADDING * 2.0)
    }

    pub fn rect(&self) -> Rect {
        let (width, height) = self.size();
        Rect::from_xywh(self.origin.0, self.origin.1, width, height)
    }

    /// The rectangle of each entry.
    pub fn entry_rects(&self) -> Vec<Rect> {
        let mut x = self.origin.0 + PANEL_PADDING;
        let y = self.origin.1 + PANEL_PADDING;
        self.entries
            .iter()
            .map(|entry| {
                let width = match entry {
                    ToolbarEntry::Button(_) => BUTTON_SIZE,
                    ToolbarEntry::Separator => SEPARATOR,
                };
                let rect = Rect::from_xywh(x, y, width, BUTTON_SIZE);
                x += width + GAP;
                rect
            })
            .collect()
    }

    /// Whether `(x, y)` is on the panel.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.rect().contains(skia_safe::Point::new(x, y))
    }

    /// The enabled button at `(x, y)`.
    pub fn button_at(&self, x: f32, y: f32) -> Option<usize> {
        self.entry_rects()
            .iter()
            .zip(&self.entries)
            .position(|(rect, entry)| {
                matches!(entry, ToolbarEntry::Button(b) if b.enabled)
                    && rect.contains(skia_safe::Point::new(x, y))
            })
    }

    /// Paints the toolbar; `progress` (0 to 1) fades and zooms it in.
    pub fn paint(&self, canvas: &Canvas, theme: &Theme, progress: f32) {
        if progress <= 0.0 {
            return;
        }
        let rect = self.rect();
        canvas.save();
        // Zoom from 95% around the center, as shadcn's popovers open.
        let zoom = 0.95 + 0.05 * progress;
        canvas.translate((rect.center_x(), rect.center_y()));
        canvas.scale((zoom, zoom));
        canvas.translate((-rect.center_x(), -rect.center_y()));
        paint_panel(canvas, rect, theme, progress);
        let palette = &theme.palette;
        for (index, (entry, entry_rect)) in self.entries.iter().zip(self.entry_rects()).enumerate()
        {
            match entry {
                ToolbarEntry::Separator => {
                    let mut line = Paint::default();
                    line.set_color(with_alpha(sk(palette.border), progress));
                    canvas.draw_rect(
                        Rect::from_xywh(
                            entry_rect.center_x() - 0.5,
                            entry_rect.top + 6.0,
                            1.0,
                            entry_rect.height() - 12.0,
                        ),
                        &line,
                    );
                }
                ToolbarEntry::Button(button) => {
                    let background = if button.active {
                        Some(palette.accent)
                    } else if button.enabled
                        && (self.hovered == Some(index) || self.pressed == Some(index))
                    {
                        Some(palette.muted)
                    } else {
                        None
                    };
                    if let Some(color) = background {
                        let mut fill = Paint::default();
                        fill.set_anti_alias(true);
                        fill.set_color(with_alpha(sk(color), progress));
                        canvas.draw_rrect(
                            RRect::new_rect_xy(entry_rect, theme.radius.sm, theme.radius.sm),
                            &fill,
                        );
                    }
                    let color = if button.active {
                        palette.accent_foreground
                    } else {
                        palette.popover_foreground
                    };
                    let opacity = if button.enabled {
                        progress
                    } else {
                        progress * 0.5
                    };
                    button.icon.draw(
                        canvas,
                        entry_rect.center_x() - ICON_SIZE / 2.0,
                        entry_rect.center_y() - ICON_SIZE / 2.0,
                        ICON_SIZE,
                        with_alpha(sk(color), opacity),
                    );
                }
            }
        }
        canvas.restore();
    }
}
