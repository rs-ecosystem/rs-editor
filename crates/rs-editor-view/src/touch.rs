//! Touch interaction that feels like the platform's own text fields:
//! tap to place the caret, double/triple tap and long press to select,
//! selection handles with a magnifier while dragging, a floating
//! Cut/Copy/Paste menu and haptic ticks.
//!
//! The view decides *what* happens and paints the handles; the platform
//! adapter shows its native menu, magnifier and haptics in response to
//! [`Effect`]s, so each platform keeps its own look where users expect it.

use rs_editor_extensions::starter_kit::toggle_task_item;
use rs_editor_layout::{Affinity, Hit, Rect};
use rs_editor_state::{AllSelection, Selection, TextSelection};
use skia_safe::{Canvas, Paint, PathBuilder};

use crate::scroll::FRAME_MS;
use crate::{ClipboardContent, EditorView, Effect, PointerButton, PointerEvent, VelocityTracker};

/// Extra room around a checkbox for a fingertip, in logical pixels.
const CHECKBOX_SLACK: f32 = 10.0;

/// How handles look on the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleStyle {
    /// Android's teardrop handles hanging below the text.
    Material,
    /// iOS's lollipop handles: a bar with a knob above or below.
    Apple,
}

/// What a long press on text does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LongPressAction {
    /// Select the word, then extend by words while dragging (Android).
    SelectWord,
    /// Pick up the caret and move it with the finger under a magnifier
    /// (iOS); double tap selects words.
    MoveCaret,
}

/// Timings and sizes of touch interaction on one platform. Appearance
/// follows the platform; behavior is the same everywhere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionProfile {
    pub handle_style: HandleStyle,
    pub long_press: LongPressAction,
    pub long_press_ms: u64,
    /// Taps closer than this count as a double/triple tap.
    pub multi_tap_ms: u64,
    /// Movement (logical px) before a touch becomes a scroll.
    pub touch_slop: f32,
    /// Radius of the drawn handle.
    pub handle_radius: f32,
    /// Radius around a handle that grabs it.
    pub handle_touch_radius: f32,
    /// How long the caret handle stays after a tap.
    pub caret_handle_ms: u64,
    /// Dragging this close to the top or bottom scrolls.
    pub edge_scroll_zone: f32,
}

impl InteractionProfile {
    pub fn android() -> Self {
        Self {
            handle_style: HandleStyle::Material,
            long_press: LongPressAction::SelectWord,
            long_press_ms: 400,
            multi_tap_ms: 300,
            touch_slop: 8.0,
            handle_radius: 11.0,
            handle_touch_radius: 24.0,
            caret_handle_ms: 4_000,
            edge_scroll_zone: 40.0,
        }
    }

    pub fn ios() -> Self {
        Self {
            handle_style: HandleStyle::Apple,
            long_press: LongPressAction::MoveCaret,
            long_press_ms: 500,
            multi_tap_ms: 350,
            touch_slop: 10.0,
            handle_radius: 5.0,
            handle_touch_radius: 22.0,
            caret_handle_ms: 0,
            edge_scroll_zone: 40.0,
        }
    }
}

impl Default for InteractionProfile {
    fn default() -> Self {
        Self::android()
    }
}

/// Haptic feedback the platform should play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HapticKind {
    /// A long press started a selection.
    LongPress,
    /// A dragged handle moved to another character or word.
    TextHandleMove,
}

/// Commands of the selection menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

/// The floating selection menu to show next to `rect` (view coordinates).
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionMenu {
    pub rect: Rect,
    pub items: Vec<MenuItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

/// Which handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    Start,
    End,
    Caret,
}

/// A handle on screen: the text point it marks (view coordinates) and the
/// center of its grab area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Handle {
    pub kind: HandleKind,
    /// The caret line the handle belongs to.
    pub line: Rect,
    pub center: (f32, f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    /// Down, not yet a tap, scroll or long press.
    Pending {
        start: (f32, f32),
        down_ms: u64,
    },
    Scrolling {
        last_y: f32,
    },
    /// After a long press: dragging extends the selection by words.
    WordSelecting {
        anchor: (usize, usize),
    },
    /// After a long press with [`LongPressAction::MoveCaret`]: the caret
    /// follows the finger.
    CaretMoving,
    /// Dragging a handle; `offset` is finger minus the handle's text point.
    Handle {
        kind: HandleKind,
        offset: (f32, f32),
        /// The previous text position under the finger.
        last: Option<usize>,
        /// Where the finger went down, to tell a tap from a drag.
        start: (f32, f32),
        moved: bool,
    },
}

#[derive(Debug, Default)]
pub(crate) struct TouchState {
    gesture: Option<Gesture>,
    finger: (f32, f32),
    /// Handles are shown (the selection was made by touch).
    pub(crate) active: bool,
    caret_handle_until: u64,
    menu_shown: bool,
    /// Show the menu again once scrolling stops.
    menu_wanted: bool,
    magnifier: bool,
    last_tap: Option<(u64, f32, f32, u32)>,
    tracker: VelocityTracker,
}

impl EditorView {
    pub fn set_interaction_profile(&mut self, profile: InteractionProfile) {
        self.profile = profile;
        self.request_redraw();
    }

    pub fn interaction_profile(&self) -> InteractionProfile {
        self.profile
    }

    /// Whether a finger is down in a gesture on the text.
    pub(crate) fn touch_gesture_active(&self) -> bool {
        self.touch.gesture.is_some()
    }

    /// A touch event in view coordinates.
    pub fn touch(&mut self, phase: TouchPhase, x: f32, y: f32) {
        // Popovers and menus take touches on them first.
        if self.link_touch(phase, x, y) || self.slash_touch(phase, x, y) {
            self.request_redraw();
            return;
        }
        let now = self.clock.now_ms();
        self.touch.finger = (x, y);
        match phase {
            TouchPhase::Down => self.touch_down(x, y, now),
            TouchPhase::Move => self.touch_move(x, y, now),
            TouchPhase::Up => self.touch_up(x, y, now),
            TouchPhase::Cancel => {
                self.touch.gesture = None;
                self.set_magnifier(None);
            }
        }
        self.request_redraw();
    }

    fn touch_down(&mut self, x: f32, y: f32, now: u64) {
        self.stop_scrolling();
        self.touch.tracker.reset();
        self.touch.tracker.add(now, y);
        // A checkbox under the finger wins over a handle's touch area
        // reaching over it.
        if let Some(handle) = self
            .handle_at(x, y)
            .filter(|_| self.checkbox_under_finger(x, y).is_none())
        {
            let tip = (handle.line.x, handle.line.bottom());
            self.touch.gesture = Some(Gesture::Handle {
                kind: handle.kind,
                offset: (x - tip.0, y - tip.1),
                last: None,
                start: (x, y),
                moved: false,
            });
            return;
        }
        self.touch.gesture = Some(Gesture::Pending {
            start: (x, y),
            down_ms: now,
        });
    }

    fn touch_move(&mut self, x: f32, y: f32, now: u64) {
        match self.touch.gesture {
            Some(Gesture::Pending { start, .. }) => {
                if (x - start.0).hypot(y - start.1) > self.profile.touch_slop {
                    self.touch.menu_wanted |= self.touch.menu_shown;
                    self.hide_menu();
                    self.touch.gesture = Some(Gesture::Scrolling { last_y: start.1 });
                    self.touch_move(x, y, now);
                }
            }
            Some(Gesture::Scrolling { last_y }) => {
                self.touch.tracker.add(now, y);
                self.scroll_by(last_y - y);
                self.touch.gesture = Some(Gesture::Scrolling { last_y: y });
            }
            Some(Gesture::WordSelecting { anchor }) => self.extend_by_words(anchor, x, y),
            Some(Gesture::CaretMoving) => self.move_caret_to(x, y),
            Some(Gesture::Handle {
                kind,
                offset,
                last,
                start,
                moved,
            }) => {
                let moved = moved || (x - start.0).hypot(y - start.1) > self.profile.touch_slop;
                if !moved {
                    return;
                }
                // The drag starts: the menu steps aside until it ends.
                self.hide_menu();
                let last = self.drag_handle(kind, offset, last, x, y).or(last);
                self.touch.gesture = Some(Gesture::Handle {
                    kind,
                    offset,
                    last,
                    start,
                    moved,
                });
            }
            None => {}
        }
    }

    fn touch_up(&mut self, x: f32, y: f32, now: u64) {
        let gesture = self.touch.gesture.take();
        self.set_magnifier(None);
        match gesture {
            Some(Gesture::Pending { .. }) => self.tap(x, y, now),
            Some(Gesture::Scrolling { .. }) => {
                self.touch.tracker.add(now, y);
                let velocity = self.touch.tracker.velocity();
                self.fling(-velocity);
            }
            Some(Gesture::WordSelecting { .. }) => self.show_menu(),
            Some(Gesture::CaretMoving) => {}
            Some(Gesture::Handle { kind, moved, .. }) => {
                if kind != HandleKind::Caret {
                    self.show_menu();
                } else if moved {
                    self.touch.caret_handle_until = now + self.profile.caret_handle_ms;
                } else if self.touch.menu_shown {
                    // A tap on the caret handle toggles the Paste menu.
                    self.hide_menu();
                } else {
                    self.show_menu();
                }
            }
            None => {}
        }
    }

    /// The task item whose checkbox a finger at `x, y` touches.
    fn checkbox_under_finger(&mut self, x: f32, y: f32) -> Option<usize> {
        if !self.editor.is_editable() {
            return None;
        }
        self.checkbox_near(x, y, CHECKBOX_SLACK)
    }

    fn tap(&mut self, x: f32, y: f32, now: u64) {
        // Tapping a checkbox only toggles it: the caret, its handle and the
        // keyboard stay as they are, and quick taps do not select words.
        if let Some(pos) = self.checkbox_under_finger(x, y) {
            self.touch.last_tap = None;
            self.hide_menu();
            self.run_command(&toggle_task_item(pos));
            return;
        }
        let count = match self.touch.last_tap {
            Some((at, tx, ty, count))
                if now.saturating_sub(at) <= self.profile.multi_tap_ms
                    && (x - tx).hypot(y - ty) <= self.profile.handle_touch_radius =>
            {
                count % 3 + 1
            }
            _ => 1,
        };
        self.touch.last_tap = Some((now, x, y, count));
        let before = self.editor.selection().clone();
        let had_caret_handle = self.touch.active && before.empty();
        let menu_was_shown = self.touch.menu_shown;
        let event = PointerEvent {
            x,
            y,
            button: PointerButton::Primary,
            modifiers: Default::default(),
            click_count: count,
        };
        self.pointer_down(event);
        self.pointer_up(event);
        let after = self.editor.selection().clone();
        if self.editor.is_editable() {
            self.push_effect(Effect::ShowKeyboard);
        }
        if count > 1 && !after.empty() {
            self.touch.active = true;
            self.show_menu();
            return;
        }
        // Tapping the caret again toggles the Paste menu.
        let same_caret = had_caret_handle && after.empty() && after.head() == before.head();
        self.touch.active = matches!(after, Selection::Text(_));
        self.touch.caret_handle_until = now + self.profile.caret_handle_ms;
        if same_caret && !menu_was_shown {
            self.show_menu();
        } else {
            self.hide_menu();
        }
    }

    /// Advances timed touch behavior: long press, edge auto-scroll, the menu
    /// returning after a scroll, and the caret handle fading out.
    pub(crate) fn tick_touch(&mut self) {
        let now = self.clock.now_ms();
        match self.touch.gesture {
            Some(Gesture::Pending { start, down_ms })
                if now.saturating_sub(down_ms) >= self.profile.long_press_ms =>
            {
                self.long_press(start.0, start.1);
            }
            Some(Gesture::WordSelecting { .. })
            | Some(Gesture::CaretMoving)
            | Some(Gesture::Handle { .. }) => {
                let (x, y) = self.touch.finger;
                let zone = self.profile.edge_scroll_zone;
                let step = if y < zone {
                    -(zone - y).min(zone) * 0.3
                } else if y > self.visible_height() - zone {
                    (y - (self.visible_height() - zone)).min(zone) * 0.3
                } else {
                    0.0
                };
                if step != 0.0 {
                    self.scroll_by(step);
                    self.touch_move(x, y, now);
                    self.request_redraw();
                }
            }
            _ => {}
        }
        if self.touch.menu_wanted && self.touch.gesture.is_none() && !self.is_scrolling() {
            self.touch.menu_wanted = false;
            if self.touch.active {
                self.show_menu();
            }
        }
        if self.touch.active
            && self.editor.selection().empty()
            && !matches!(self.touch.gesture, Some(Gesture::Handle { .. }))
            && !self.touch.menu_shown
            && now >= self.touch.caret_handle_until
        {
            self.touch.active = false;
            self.request_redraw();
        }
    }

    /// Milliseconds until [`EditorView::tick_touch`] has work, if any.
    pub(crate) fn touch_deadline(&self) -> Option<u64> {
        let now = self.clock.now_ms();
        match self.touch.gesture {
            Some(Gesture::Pending { down_ms, .. }) => {
                return Some((down_ms + self.profile.long_press_ms).saturating_sub(now));
            }
            Some(Gesture::WordSelecting { .. })
            | Some(Gesture::CaretMoving)
            | Some(Gesture::Handle { .. }) => {
                let y = self.touch.finger.1;
                let zone = self.profile.edge_scroll_zone;
                if y < zone || y > self.visible_height() - zone {
                    return Some(FRAME_MS);
                }
            }
            _ => {}
        }
        if self.touch.active && self.editor.selection().empty() && !self.touch.menu_shown {
            return Some(self.touch.caret_handle_until.saturating_sub(now).max(1));
        }
        None
    }

    fn long_press(&mut self, x: f32, y: f32) {
        let Some(Hit::Text { pos, .. }) = self.hit_at(x, y) else {
            self.touch.gesture = None;
            return;
        };
        self.finish_composition();
        if self.profile.long_press == LongPressAction::MoveCaret {
            self.select_quiet(pos, pos);
            self.touch.gesture = Some(Gesture::CaretMoving);
            self.push_effect(Effect::Haptic(HapticKind::LongPress));
            if let Some(line) = self.endpoint_line(pos, Affinity::Downstream) {
                self.set_magnifier(Some((line.x, line.center_y())));
            }
            self.request_redraw();
            return;
        }
        let (from, to) = self.word_range(pos).unwrap_or((pos, pos));
        self.select_quiet(from, to);
        self.touch.active = true;
        self.touch.gesture = Some(Gesture::WordSelecting { anchor: (from, to) });
        self.push_effect(Effect::Haptic(HapticKind::LongPress));
        if let Some(line) = self.endpoint_line(to, Affinity::Upstream) {
            self.set_magnifier(Some((line.x, line.center_y())));
        }
        self.request_redraw();
    }

    fn move_caret_to(&mut self, x: f32, y: f32) {
        let Some(Hit::Text { pos, affinity }) = self.hit_at(x, y) else {
            return;
        };
        self.select_with_tick(pos, pos);
        self.affinity = affinity;
        if let Some(line) = self.endpoint_line(pos, affinity) {
            self.set_magnifier(Some((line.x, line.center_y())));
        }
    }

    fn extend_by_words(&mut self, anchor: (usize, usize), x: f32, y: f32) {
        let Some(Hit::Text { pos, .. }) = self.hit_at(x, y) else {
            return;
        };
        let (word_from, word_to) = self.word_range(pos).unwrap_or((pos, pos));
        let (selection_anchor, head) = if pos < anchor.0 {
            (anchor.1, word_from)
        } else if pos > anchor.1 {
            (anchor.0, word_to)
        } else {
            (anchor.0, anchor.1)
        };
        self.select_with_tick(selection_anchor, head);
        if let Some(line) = self.endpoint_line(head, Affinity::Upstream) {
            self.set_magnifier(Some((line.x, line.center_y())));
        }
    }

    /// Moves a handle under the finger; returns the text position the
    /// finger is over.
    fn drag_handle(
        &mut self,
        kind: HandleKind,
        offset: (f32, f32),
        last: Option<usize>,
        x: f32,
        y: f32,
    ) -> Option<usize> {
        let tip = (x - offset.0, y - offset.1);
        // The handle hangs below its line; aim just above the tip.
        let Some(Hit::Text { pos, .. }) = self.hit_at(tip.0, tip.1 - 2.0) else {
            return None;
        };
        let selection = self.editor.selection().clone();
        let (from, to) = (selection.from(), selection.to());
        // Android's rule: moving outward never shrinks the selection and
        // jumps to whole words; moving back moves by character.
        let (anchor, head) = match kind {
            HandleKind::Caret => (pos, pos),
            HandleKind::Start => {
                if pos >= to {
                    return Some(pos);
                }
                let outward = last.is_none_or(|last| pos <= last);
                let head = if !outward {
                    pos
                } else if pos < from {
                    self.word_range(pos)
                        .map_or(pos, |(start, _)| start.min(pos))
                } else {
                    from
                };
                (to, head)
            }
            HandleKind::End => {
                if pos <= from {
                    return Some(pos);
                }
                let outward = last.is_none_or(|last| pos >= last);
                let head = if !outward {
                    pos
                } else if pos > to {
                    self.word_range(pos).map_or(pos, |(_, end)| end.max(pos))
                } else {
                    to
                };
                (from, head)
            }
        };
        self.select_with_tick(anchor, head);
        let affinity = if kind == HandleKind::End {
            Affinity::Upstream
        } else {
            Affinity::Downstream
        };
        if let Some(line) = self.endpoint_line(head, affinity) {
            self.set_magnifier(Some((line.x, line.center_y())));
        }
        Some(pos)
    }

    /// Selects without scrolling, with a haptic tick when it changed.
    fn select_with_tick(&mut self, anchor: usize, head: usize) {
        let before = self.editor.selection().clone();
        if before.anchor() == anchor && before.head() == head {
            return;
        }
        self.select_quiet(anchor, head);
        self.push_effect(Effect::Haptic(HapticKind::TextHandleMove));
    }

    fn select_quiet(&mut self, anchor: usize, head: usize) {
        let doc = self.editor.doc().clone();
        let (Ok(a), Ok(h)) = (doc.resolve(anchor), doc.resolve(head)) else {
            return;
        };
        let mut tr = self.editor.state().tr();
        tr.set_selection(TextSelection::between(&a, &h, None));
        self.dispatch(tr);
    }

    /// The caret line at a selection endpoint, in view coordinates.
    fn endpoint_line(&mut self, pos: usize, affinity: Affinity) -> Option<Rect> {
        let rect = self.layout().caret_rect(pos, affinity, 0.0)?;
        Some(self.to_view(rect))
    }

    /// The handles currently shown.
    pub fn touch_handles(&mut self) -> Vec<Handle> {
        // The link address field has the input; the text's handles step aside.
        if !self.touch.active || !self.focused || self.link_editing() {
            return Vec::new();
        }
        let selection = self.editor.selection().clone();
        if !matches!(selection, Selection::Text(_) | Selection::All(_)) {
            return Vec::new();
        }
        let now = self.clock.now_ms();
        let dragging_caret = matches!(
            self.touch.gesture,
            Some(Gesture::Handle {
                kind: HandleKind::Caret,
                ..
            })
        );
        let ends: Vec<(HandleKind, usize, Affinity)> = if selection.empty() {
            if now < self.touch.caret_handle_until || dragging_caret || self.touch.menu_shown {
                vec![(HandleKind::Caret, selection.head(), self.affinity)]
            } else {
                Vec::new()
            }
        } else {
            // Ends outside text (a whole-document selection) sit at the
            // nearest text position.
            let doc = self.editor.doc().clone();
            let inside = |pos: usize, dir: i8| {
                doc.resolve(pos)
                    .ok()
                    .map_or(pos, |at| Selection::near(&at, dir).head())
            };
            vec![
                (
                    HandleKind::Start,
                    inside(selection.from(), 1),
                    Affinity::Downstream,
                ),
                (
                    HandleKind::End,
                    inside(selection.to(), -1),
                    Affinity::Upstream,
                ),
            ]
        };
        let r = self.profile.handle_radius;
        ends.into_iter()
            .filter_map(|(kind, pos, affinity)| {
                let line = self.endpoint_line(pos, affinity)?;
                let center = match (self.profile.handle_style, kind) {
                    (HandleStyle::Material, HandleKind::Start) => (line.x - r, line.bottom() + r),
                    (HandleStyle::Material, HandleKind::End) => (line.x + r, line.bottom() + r),
                    (HandleStyle::Material, HandleKind::Caret) => {
                        (line.x, line.bottom() + r * std::f32::consts::SQRT_2)
                    }
                    (HandleStyle::Apple, HandleKind::Start) => (line.x, line.y - r),
                    (HandleStyle::Apple, _) => (line.x, line.bottom() + r),
                };
                Some(Handle { kind, line, center })
            })
            .collect()
    }

    fn handle_at(&mut self, x: f32, y: f32) -> Option<Handle> {
        let reach = self.profile.handle_touch_radius;
        self.touch_handles()
            .into_iter()
            .map(|h| ((h.center.0 - x).hypot(h.center.1 - y), h))
            .filter(|(distance, _)| *distance <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, h)| h)
    }

    // ---- menu and magnifier ---------------------------------------------

    fn menu(&mut self) -> Option<SelectionMenu> {
        let selection = self.editor.selection().clone();
        let editable = self.editor.is_editable();
        let doc_size = self.editor.doc().content().size();
        let all = matches!(selection, Selection::All(_))
            || (selection.from() <= 1 && selection.to() + 1 >= doc_size && !selection.empty());
        let mut items = Vec::new();
        let rect = if selection.empty() {
            if editable {
                items.push(MenuItem::Paste);
            }
            self.caret_rect()?
        } else {
            if editable {
                items.push(MenuItem::Cut);
            }
            items.push(MenuItem::Copy);
            if editable {
                items.push(MenuItem::Paste);
            }
            let rects = self
                .layout()
                .selection_rects(selection.from(), selection.to());
            let mut bounds = rects.first().copied()?;
            for r in &rects[1..] {
                let (x0, y0) = (bounds.x.min(r.x), bounds.y.min(r.y));
                let (x1, y1) = (
                    bounds.right().max(r.right()),
                    bounds.bottom().max(r.bottom()),
                );
                bounds = Rect::new(x0, y0, x1 - x0, y1 - y0);
            }
            self.to_view(bounds)
        };
        if !all && !self.editor.is_empty() {
            items.push(MenuItem::SelectAll);
        }
        (!items.is_empty()).then_some(SelectionMenu { rect, items })
    }

    fn show_menu(&mut self) {
        match self.menu() {
            Some(menu) => {
                self.touch.menu_shown = true;
                self.push_effect(Effect::SelectionMenu(Some(menu)));
            }
            None => self.hide_menu(),
        }
    }

    pub(crate) fn hide_menu(&mut self) {
        if self.touch.menu_shown {
            self.touch.menu_shown = false;
            self.push_effect(Effect::SelectionMenu(None));
        }
    }

    fn set_magnifier(&mut self, at: Option<(f32, f32)>) {
        if at.is_none() && !self.touch.magnifier {
            return;
        }
        self.touch.magnifier = at.is_some();
        self.push_effect(Effect::Magnifier(at));
    }

    /// Runs a command chosen from the selection menu. For Paste, `text`
    /// and `html` are the platform clipboard's contents (HTML wins). Copy
    /// and Cut return what to put on the clipboard.
    pub fn menu_action(
        &mut self,
        item: MenuItem,
        text: Option<&str>,
        html: Option<&str>,
    ) -> Option<ClipboardContent> {
        self.hide_menu();
        match item {
            MenuItem::Copy => {
                let copied = self.copy()?;
                let to = self.editor.selection().to();
                self.select_quiet(to, to);
                self.touch.active = false;
                Some(copied)
            }
            MenuItem::Cut => {
                self.touch.active = false;
                self.cut()
            }
            MenuItem::Paste => {
                let pasted = html
                    .filter(|h| !h.trim().is_empty())
                    .is_some_and(|h| self.paste_html(h));
                if !pasted && let Some(text) = text.filter(|t| !t.is_empty()) {
                    self.paste_text(text);
                }
                self.touch.active = false;
                None
            }
            MenuItem::SelectAll => {
                let doc = self.editor.doc().clone();
                let mut tr = self.editor.state().tr();
                tr.set_selection(Selection::All(AllSelection::new(&doc)));
                self.dispatch(tr);
                self.touch.active = true;
                self.show_menu();
                None
            }
        }
    }

    /// The platform closed the selection menu on its own (back button, a
    /// tap elsewhere in the system UI).
    pub fn selection_menu_dismissed(&mut self) {
        self.touch.menu_shown = false;
        self.touch.menu_wanted = false;
    }

    /// Typing, keys or a mouse end touch selection mode.
    pub(crate) fn end_touch_mode(&mut self) {
        if self.touch.active || self.touch.menu_shown {
            self.touch.active = false;
            self.hide_menu();
            self.request_redraw();
        }
        self.set_magnifier(None);
    }

    /// Whether a touch drag is in progress (hosts may stop parent scrolling).
    pub fn is_touch_dragging(&self) -> bool {
        matches!(
            self.touch.gesture,
            Some(Gesture::Handle { .. } | Gesture::WordSelecting { .. } | Gesture::CaretMoving)
        )
    }

    pub(crate) fn paint_handles(&mut self, canvas: &Canvas) {
        let handles = self.touch_handles();
        if handles.is_empty() {
            return;
        }
        let color = self.theme().palette.selection.with_alpha(1.0);
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(skia_safe::Color::new(color.argb()));
        let r = self.profile.handle_radius;
        for handle in handles {
            let (x, bottom) = (handle.line.x, handle.line.bottom());
            match self.profile.handle_style {
                HandleStyle::Material => {
                    let (cx, cy) = handle.center;
                    canvas.draw_circle((cx, cy), r, &paint);
                    match handle.kind {
                        HandleKind::Start => {
                            canvas
                                .draw_rect(skia_safe::Rect::from_xywh(x - r, bottom, r, r), &paint);
                        }
                        HandleKind::End => {
                            canvas.draw_rect(skia_safe::Rect::from_xywh(x, bottom, r, r), &paint);
                        }
                        HandleKind::Caret => {
                            let half = r * std::f32::consts::FRAC_1_SQRT_2;
                            let mut path = PathBuilder::new();
                            path.move_to((x, bottom));
                            path.line_to((x + half, bottom + half));
                            path.line_to((x, cy));
                            path.line_to((x - half, bottom + half));
                            path.close();
                            canvas.draw_path(&path.detach(), &paint);
                        }
                    }
                }
                HandleStyle::Apple => {
                    let line = handle.line;
                    canvas.draw_rect(
                        skia_safe::Rect::from_xywh(x - 1.0, line.y, 2.0, line.height),
                        &paint,
                    );
                    if handle.kind != HandleKind::Caret {
                        canvas.draw_circle(handle.center, r, &paint);
                    }
                }
            }
        }
    }
}
