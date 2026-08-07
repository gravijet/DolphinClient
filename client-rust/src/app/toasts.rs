//! Toasts: the little sliding cards in the top-right corner.
//!
//! Vanilla shows one whenever something happened that is worth knowing about
//! but not worth interrupting you for — an advancement completed, new recipes
//! unlocked, the tutorial nudging you, the disc that just started playing. Each
//! one slides in from the right edge over 600 ms, sits for five seconds, and
//! slides back out; several stack downward.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Id, LayerId, Order, Rect, pos2, vec2};

use crate::app::container;
use crate::app::mcui::McUi;
use crate::assets::items::ItemIcons;
use crate::bridge::events::{ChatSpan, ItemSnapshot};

/// Vanilla's toast width in GUI pixels; the sprites are 160 wide.
const WIDTH: f32 = 160.0;
/// One toast slot. The system toast is two slots tall.
const SLOT: f32 = 32.0;
/// How long the slide in and the slide out each take.
const SLIDE: Duration = Duration::from_millis(600);
/// Vanilla's default dwell time.
const DWELL: Duration = Duration::from_millis(5000);
/// Vanilla never shows more than five at once; the rest wait their turn.
const MAX_VISIBLE: usize = 5;

/// One card waiting to be shown or already showing.
#[derive(Clone)]
pub struct Toast {
    /// Background sprite name under `gui/sprites/toast/`.
    pub background: &'static str,
    /// How many 32-px slots the background covers (the system toast is 2).
    pub slots: usize,
    /// Top line, drawn in the colour the kind calls for.
    pub title: String,
    pub title_color: Color32,
    /// Second line (and, on the system toast, a third).
    pub lines: Vec<String>,
    /// Item drawn in the 16×16 icon well on the left.
    pub item: Option<ItemSnapshot>,
    /// …or a 20×20 sprite from the toast sheet, for the tutorial cards.
    pub sprite: Option<&'static str>,
    /// How long it stays once fully on screen.
    pub dwell: Duration,
}

impl Toast {
    /// An advancement was completed. Vanilla titles it by the frame kind and
    /// colours that title green, or purple for a challenge.
    pub fn advancement(frame: u8, name: Vec<ChatSpan>, icon: Option<ItemSnapshot>, lang_title: &str) -> Self {
        Self {
            background: "advancement",
            slots: 1,
            title: lang_title.to_string(),
            title_color: if frame == 1 {
                // Challenge — vanilla's dark purple.
                Color32::from_rgb(0xAA, 0x00, 0xAA)
            } else {
                Color32::from_rgb(0x55, 0xFF, 0x55)
            },
            lines: vec![crate::bridge::events::spans_to_plain(&name)],
            item: icon,
            sprite: None,
            dwell: DWELL,
        }
    }

    /// A plain informational card (the "system" background, two lines).
    pub fn system(title: impl Into<String>, body: impl Into<String>) -> Self {
        let body = body.into();
        Self {
            background: "system",
            slots: 2,
            title: title.into(),
            title_color: Color32::WHITE,
            lines: if body.is_empty() { Vec::new() } else { vec![body] },
            item: None,
            sprite: None,
            dwell: Duration::from_millis(5000),
        }
    }

    /// New recipes unlocked — vanilla shows the recipe-book icon.
    pub fn recipe(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            background: "recipe",
            slots: 1,
            title: title.into(),
            title_color: Color32::from_rgb(0xFF, 0xFF, 0x55),
            lines: vec![body.into()],
            item: None,
            sprite: Some("recipe_book"),
            dwell: DWELL,
        }
    }

    /// The "Now Playing" card a jukebox puts up.
    pub fn now_playing(track: impl Into<String>) -> Self {
        Self {
            background: "now_playing",
            slots: 1,
            title: "Now Playing:".to_string(),
            title_color: Color32::from_rgb(0xFF, 0xFF, 0x55),
            lines: vec![track.into()],
            item: None,
            sprite: None,
            dwell: Duration::from_millis(5000),
        }
    }
}

struct Showing {
    toast: Toast,
    since: Instant,
    /// Which slot row it occupies, top to bottom.
    row: usize,
}

/// The toast queue: what is on screen and what is waiting.
#[derive(Default)]
pub struct Toasts {
    pending: VecDeque<Toast>,
    showing: Vec<Showing>,
}

impl Toasts {
    pub fn push(&mut self, toast: Toast) {
        // A flood of advancements on join would otherwise queue for minutes.
        if self.pending.len() < 32 {
            self.pending.push_back(toast);
        }
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.showing.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.showing.is_empty()
    }

    /// Put every queued card straight into its settled position. Only the
    /// deterministic screenshot dump uses this: a single rendered frame would
    /// otherwise catch every toast at the very start of its slide-in, i.e.
    /// still off the right edge.
    pub fn settle(&mut self) {
        let now = Instant::now();
        self.advance(now);
        for show in &mut self.showing {
            show.since = now.checked_sub(SLIDE).unwrap_or(now);
        }
    }

    /// Retire finished cards and promote waiting ones into the free rows.
    fn advance(&mut self, now: Instant) {
        self.showing.retain(|s| {
            now.duration_since(s.since) < SLIDE + s.toast.dwell + SLIDE
        });
        while !self.pending.is_empty() {
            let used: usize = self.showing.iter().map(|s| s.toast.slots).sum();
            let Some(next) = self.pending.front() else { break };
            if used + next.slots > MAX_VISIBLE {
                break;
            }
            let toast = self.pending.pop_front().expect("front checked above");
            self.showing.push(Showing { toast, since: now, row: used });
        }
    }

    /// Draw the stack in the top-right corner. `s` is the GUI scale.
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        icons: &Option<(egui::TextureId, std::sync::Arc<ItemIcons>)>,
    ) {
        let now = Instant::now();
        self.advance(now);
        if self.showing.is_empty() {
            return;
        }
        // Toasts sit above everything else — they are meant to be seen even
        // with a container screen open.
        let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("toasts")));
        let screen = ctx.content_rect();
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        for show in &self.showing {
            let age = now.duration_since(show.since);
            // Slide in, hold, slide back out — 0 fully off screen, 1 fully on.
            let visible = if age < SLIDE {
                age.as_secs_f32() / SLIDE.as_secs_f32()
            } else if age < SLIDE + show.toast.dwell {
                1.0
            } else {
                1.0 - (age - SLIDE - show.toast.dwell).as_secs_f32() / SLIDE.as_secs_f32()
            }
            .clamp(0.0, 1.0);
            let h = SLOT * show.toast.slots as f32;
            let x = screen.right() - WIDTH * s * visible;
            let y = screen.top() + show.row as f32 * SLOT * s;
            let rect = Rect::from_min_size(pos2(x, y), vec2(WIDTH * s, h * s));
            if let Some(tex) = mc.tex.toast.get(show.toast.background) {
                painter.image(tex.id(), rect, full, Color32::WHITE);
            }
            // Icon well: an item stack, or one of the tutorial sprites.
            if let Some(item) = &show.toast.item {
                let cell = Rect::from_min_size(rect.min + vec2(8.0 * s, 8.0 * s), vec2(16.0 * s, 16.0 * s));
                container::draw_item(&painter, mc, icons, cell, item, s);
            } else if let Some(name) = show.toast.sprite
                && let Some(tex) = mc.tex.toast.get(name)
            {
                let cell = Rect::from_min_size(rect.min + vec2(6.0 * s, 6.0 * s), vec2(20.0 * s, 20.0 * s));
                painter.image(tex.id(), cell, full, Color32::WHITE);
            }
            let text_x = rect.min.x + 30.0 * s;
            mc.font.draw_anchored(
                &painter,
                pos2(text_x, rect.min.y + 7.0 * s),
                Align2::LEFT_TOP,
                &show.toast.title,
                s,
                show.toast.title_color,
                false,
            );
            for (i, line) in show.toast.lines.iter().enumerate() {
                mc.font.draw_anchored(
                    &painter,
                    pos2(text_x, rect.min.y + (18.0 + i as f32 * 11.0) * s),
                    Align2::LEFT_TOP,
                    line,
                    s,
                    Color32::WHITE,
                    false,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy() -> Toast {
        Toast::system("Title", "Body")
    }

    #[test]
    fn only_five_slots_are_used_at_once() {
        let mut toasts = Toasts::default();
        for _ in 0..6 {
            toasts.push(dummy()); // two slots each
        }
        toasts.advance(Instant::now());
        // Two two-slot toasts fit; a third would need six slots.
        assert_eq!(toasts.showing.len(), 2);
        assert_eq!(toasts.pending.len(), 4);
    }

    #[test]
    fn rows_stack_downward_without_overlapping() {
        let mut toasts = Toasts::default();
        toasts.push(Toast::recipe("a", "b"));
        toasts.push(Toast::recipe("c", "d"));
        toasts.advance(Instant::now());
        assert_eq!(toasts.showing[0].row, 0);
        assert_eq!(toasts.showing[1].row, 1);
    }

    #[test]
    fn a_finished_toast_makes_room_for_the_next() {
        let mut toasts = Toasts::default();
        for _ in 0..3 {
            toasts.push(dummy());
        }
        let start = Instant::now();
        toasts.advance(start);
        assert_eq!(toasts.showing.len(), 2);
        // Long past slide-in + dwell + slide-out, the first two are gone and
        // the third takes their place.
        toasts.advance(start + SLIDE + DWELL + SLIDE + Duration::from_millis(1));
        assert_eq!(toasts.showing.len(), 1);
        assert!(toasts.pending.is_empty());
    }

    #[test]
    fn the_queue_does_not_grow_without_bound() {
        let mut toasts = Toasts::default();
        for _ in 0..100 {
            toasts.push(dummy());
        }
        assert_eq!(toasts.pending.len(), 32);
    }

    #[test]
    fn a_challenge_toast_is_purple() {
        let task = Toast::advancement(0, vec![ChatSpan::plain("x")], None, "Advancement Made!");
        let challenge = Toast::advancement(1, vec![ChatSpan::plain("x")], None, "Challenge Complete!");
        assert_ne!(task.title_color, challenge.title_color);
        assert_eq!(challenge.title_color, Color32::from_rgb(0xAA, 0x00, 0xAA));
    }
}
