//! Appearing and disappearing, as shadcn's popovers do: a quick fade with
//! a slight zoom.

/// The progress of a panel showing (`1.0`) or hiding (`0.0`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Appear {
    from: f32,
    to: f32,
    start_ms: u64,
    duration_ms: u64,
}

impl Default for Appear {
    fn default() -> Self {
        Self {
            from: 0.0,
            to: 0.0,
            start_ms: 0,
            duration_ms: 1,
        }
    }
}

impl Appear {
    pub const SHOW_MS: u64 = 150;
    pub const HIDE_MS: u64 = 100;

    /// Starts showing (`visible`) or hiding at `now`, from wherever it is.
    pub fn set(&mut self, visible: bool, now: u64) {
        let target = if visible { 1.0 } else { 0.0 };
        if self.to == target {
            return;
        }
        self.from = self.value(now);
        self.to = target;
        self.start_ms = now;
        self.duration_ms = if visible {
            Self::SHOW_MS
        } else {
            Self::HIDE_MS
        };
    }

    /// Shows or hides at once.
    pub fn jump(&mut self, visible: bool) {
        *self = Self {
            from: if visible { 1.0 } else { 0.0 },
            to: if visible { 1.0 } else { 0.0 },
            ..Self::default()
        };
    }

    pub fn target(&self) -> bool {
        self.to > 0.5
    }

    /// Eased progress at `now`, 0 hidden to 1 shown.
    pub fn value(&self, now: u64) -> f32 {
        let t = (now.saturating_sub(self.start_ms) as f32 / self.duration_ms as f32).min(1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from + (self.to - self.from) * eased
    }

    /// Whether frames are still needed at `now`.
    pub fn animating(&self, now: u64) -> bool {
        now.saturating_sub(self.start_ms) < self.duration_ms && self.from != self.to
    }

    /// Whether anything shows at `now`.
    pub fn visible(&self, now: u64) -> bool {
        self.value(now) > 0.0
    }
}
