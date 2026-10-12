//! Animated scrolling: smooth steps for wheels and keys, momentum after a
//! fling. Animations move the view by deltas, so they stay correct when
//! scroll anchoring shifts the offset underneath them.

use crate::EditorView;

/// Interval between animation frames when the host has no vsync signal.
pub const FRAME_MS: u64 = 16;

/// Momentum keeps this fraction of its velocity per millisecond (the
/// deceleration of iOS scroll views).
const DECELERATION: f32 = 0.998;
/// Momentum stops below this speed (px/ms).
const MIN_VELOCITY: f32 = 0.02;
/// Duration of a smooth step.
const SMOOTH_MS: f32 = 180.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Scroll {
    Idle,
    /// Momentum in px/ms.
    Fling {
        velocity: f32,
        last_ms: u64,
    },
    /// An eased move by `total` started at `start_ms`, `applied` so far.
    Smooth {
        total: f32,
        applied: f32,
        start_ms: u64,
    },
}

/// Estimates pointer velocity from recent positions (least squares over the
/// last 100 ms), for starting a fling when a touch drag ends.
#[derive(Debug, Clone, Default)]
pub struct VelocityTracker {
    samples: Vec<(u64, f32)>,
}

impl VelocityTracker {
    const WINDOW_MS: u64 = 100;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.samples.clear();
    }

    /// Records the pointer at `y` (view coordinates) at `time_ms`.
    pub fn add(&mut self, time_ms: u64, y: f32) {
        self.samples.push((time_ms, y));
        let cutoff = time_ms.saturating_sub(Self::WINDOW_MS);
        self.samples.retain(|&(t, _)| t >= cutoff);
    }

    /// Velocity in px/s; positive when the pointer moves down.
    pub fn velocity(&self) -> f32 {
        let n = self.samples.len();
        if n < 2 {
            return 0.0;
        }
        let t0 = self.samples[0].0;
        let mean_t = self
            .samples
            .iter()
            .map(|&(t, _)| (t - t0) as f32)
            .sum::<f32>()
            / n as f32;
        let mean_y = self.samples.iter().map(|&(_, y)| y).sum::<f32>() / n as f32;
        let (mut cov, mut var) = (0.0, 0.0);
        for &(t, y) in &self.samples {
            let dt = (t - t0) as f32 - mean_t;
            cov += dt * (y - mean_y);
            var += dt * dt;
        }
        if var == 0.0 { 0.0 } else { cov / var * 1000.0 }
    }
}

impl EditorView {
    /// Scrolls by `dy` with a short ease-out animation (mouse wheel notches,
    /// keyboard). Steps arriving mid-animation add up.
    pub fn scroll_smooth_by(&mut self, dy: f32) {
        let now = self.clock.now_ms();
        let remaining = match self.scroll {
            Scroll::Smooth { total, applied, .. } => total - applied,
            _ => 0.0,
        };
        self.scroll = Scroll::Smooth {
            total: remaining + dy,
            applied: 0.0,
            start_ms: now,
        };
        self.request_redraw();
    }

    /// Starts momentum scrolling at `velocity` px/s (positive scrolls the
    /// content up, toward the end of the document), as after a touch fling.
    pub fn fling(&mut self, velocity: f32) {
        let velocity = velocity / 1000.0;
        self.scroll = if velocity.abs() < MIN_VELOCITY {
            Scroll::Idle
        } else {
            Scroll::Fling {
                velocity,
                last_ms: self.clock.now_ms(),
            }
        };
        self.request_redraw();
    }

    /// Stops any scroll animation (a touch or click on the content).
    pub fn stop_scrolling(&mut self) {
        self.scroll = Scroll::Idle;
    }

    pub fn is_scrolling(&self) -> bool {
        self.scroll != Scroll::Idle
    }

    /// Advances the scroll animation to the clock's time.
    pub(crate) fn tick_scroll(&mut self) {
        let now = self.clock.now_ms();
        match self.scroll {
            Scroll::Idle => {}
            Scroll::Smooth {
                total,
                applied,
                start_ms,
            } => {
                let t = ((now.saturating_sub(start_ms)) as f32 / SMOOTH_MS).min(1.0);
                let eased = 1.0 - (1.0 - t).powi(3);
                let target = total * eased;
                let moved = self.move_scroll(target - applied);
                self.scroll = if t >= 1.0 || !moved {
                    Scroll::Idle
                } else {
                    Scroll::Smooth {
                        total,
                        applied: target,
                        start_ms,
                    }
                };
                self.request_redraw();
            }
            Scroll::Fling { velocity, last_ms } => {
                let dt = now.saturating_sub(last_ms) as f32;
                if dt == 0.0 {
                    return;
                }
                // Distance under exponential decay: v·τ·(1 − e^(−dt/τ)).
                let decay = DECELERATION.powf(dt);
                let tau = -1.0 / DECELERATION.ln();
                let distance = velocity * tau * (1.0 - decay);
                let velocity = velocity * decay;
                let moved = self.move_scroll(distance);
                self.scroll = if !moved || velocity.abs() < MIN_VELOCITY {
                    Scroll::Idle
                } else {
                    Scroll::Fling {
                        velocity,
                        last_ms: now,
                    }
                };
                self.request_redraw();
            }
        }
    }

    /// Moves the offset; returns whether it changed (false at the edges).
    fn move_scroll(&mut self, dy: f32) -> bool {
        let before = self.scroll_y;
        self.scroll_to_inner(self.scroll_y + dy);
        (self.scroll_y - before).abs() > f32::EPSILON || dy.abs() < f32::EPSILON
    }
}
