//! In-game chat: colored/styled log lines, clickable links & commands,
//! Minecraft-font input line with history, scrolling and server-side command
//! tab-completion — laid out like vanilla.

use std::collections::VecDeque;
use std::time::Instant;

use egui::{Color32, Id, Key, LayerId, Order, Rect, pos2, vec2};

use crate::app::hud::HudAction;
use crate::app::mcui::{self, LINE_H, McUi};
use crate::bridge::events::{ChatClick, ChatSpan};
use crate::settings::{ChatVisibility, GameSettings};

/// Chat lines older than this fade out (when the chat is closed).
const CHAT_VISIBLE_SECS: f32 = 15.0;
/// Kept log length (vanilla keeps 100).
const CHAT_MAX_LINES: usize = 100;
/// Visible rows when closed / open.
const ROWS_CLOSED: usize = 10;
const ROWS_OPEN: usize = 20;

struct ChatLine {
    spans: Vec<ChatSpan>,
    when: Instant,
    /// System/command feedback (shown in "Commands Only" visibility).
    system: bool,
}

/// Active command-completion suggestions.
pub struct Suggestions {
    /// Byte range of `input` the entries replace.
    pub start: usize,
    pub length: usize,
    pub entries: Vec<String>,
    pub selected: usize,
}

pub struct ChatState {
    pub open: bool,
    /// Set on the frame the chat was opened — swallows the opening keypress.
    just_opened: bool,
    lines: VecDeque<ChatLine>,
    pub input: String,
    cursor: usize,
    history: Vec<String>,
    /// `None` = typing a fresh line; `Some(i)` = browsing history at `i`.
    history_pos: Option<usize>,
    draft: String,
    /// Lines scrolled up from the newest.
    scroll: usize,
    sugg: Option<Suggestions>,
    next_req_id: u32,
    /// Outstanding completion request (id, text it was for).
    pending_req: Option<(u32, String)>,
    /// Input text the last request was made for (avoid re-requesting).
    last_requested: String,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            open: false,
            just_opened: false,
            lines: VecDeque::new(),
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            draft: String::new(),
            scroll: 0,
            sugg: None,
            next_req_id: 1,
            pending_req: None,
            last_requested: String::new(),
        }
    }
}

impl ChatState {
    pub fn push(&mut self, spans: Vec<ChatSpan>, system: bool) {
        self.lines.push_back(ChatLine { spans, when: Instant::now(), system });
        while self.lines.len() > CHAT_MAX_LINES {
            self.lines.pop_front();
        }
        if self.scroll > 0 {
            self.scroll += 1; // keep the viewport anchored while scrolled up
        }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.close();
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Open the chat with `prefill` (empty, "/" for the command key, or a
    /// suggested command from a click event).
    pub fn open_with(&mut self, prefill: &str) {
        self.open = true;
        self.just_opened = true;
        self.input = prefill.to_string();
        self.cursor = self.input.len();
        self.history_pos = None;
        self.scroll = 0;
        self.sugg = None;
        self.last_requested.clear();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.input.clear();
        self.cursor = 0;
        self.history_pos = None;
        self.scroll = 0;
        self.sugg = None;
        self.pending_req = None;
        self.last_requested.clear();
    }

    /// Server answered a completion request.
    pub fn on_suggestions(&mut self, id: u32, start: usize, length: usize, entries: Vec<String>) {
        if self.pending_req.as_ref().map(|(i, _)| *i) != Some(id) || !self.open {
            return;
        }
        self.pending_req = None;
        if entries.is_empty() {
            self.sugg = None;
        } else {
            self.sugg = Some(Suggestions { start, length, entries, selected: 0 });
        }
    }

    /// Draw log + input + suggestions; translate interactions into actions.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        settings: &GameSettings,
        actions: &mut Vec<HudAction>,
    ) {
        let cs = s * settings.chat_scale.clamp(0.5, 2.0);
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("chat-log")));
        let r = ctx.content_rect();
        let max_w = settings.chat_width.clamp(40.0, 320.0) * cs;
        let line_h = LINE_H * cs * settings.chat_line_spacing.max(1.0);
        let time = ctx.input(|i| i.time);

        // --- input handling (before drawing so this frame reflects it) -------
        if self.open {
            self.handle_input(ctx, settings.command_suggestions, actions);
        }

        // --- collect visible wrapped rows, newest first -----------------------
        let show_all = self.open;
        let mut rows: Vec<(Vec<ChatSpan>, f32)> = Vec::new(); // (spans, alpha), newest first
        if settings.chat_visibility != ChatVisibility::Hidden {
            'lines: for line in self.lines.iter().rev() {
                if settings.chat_visibility == ChatVisibility::System && !line.system {
                    continue;
                }
                let age = line.when.elapsed().as_secs_f32();
                let alpha = if show_all {
                    1.0
                } else if age >= CHAT_VISIBLE_SECS {
                    continue;
                } else {
                    ((CHAT_VISIBLE_SECS - age) / 2.0).clamp(0.0, 1.0)
                };
                for piece in wrap_spans(mc, &line.spans, cs, max_w - 4.0 * cs).into_iter().rev() {
                    rows.push((piece, alpha));
                    if rows.len() >= ROWS_OPEN + self.scroll + 1 {
                        break 'lines;
                    }
                }
            }
        }

        let max_rows = if self.open { ROWS_OPEN } else { ROWS_CLOSED };
        self.scroll = self.scroll.min(rows.len().saturating_sub(1));
        let base_y = r.bottom() - 48.0 * s;
        let opacity = settings.chat_opacity.clamp(0.0, 1.0);
        let pointer = ctx.pointer_latest_pos();
        let clicked = ctx.input(|i| i.pointer.primary_clicked());

        // Mouse-wheel scrolling over the chat area while open.
        if self.open {
            let scroll_delta = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll_delta.abs() > 0.5
                && let Some(p) = pointer
                && p.y > base_y - max_rows as f32 * line_h
                && p.y < base_y
                && p.x < r.left() + max_w
            {
                if scroll_delta > 0.0 {
                    self.scroll = (self.scroll + 3).min(rows.len().saturating_sub(1));
                } else {
                    self.scroll = self.scroll.saturating_sub(3);
                }
            }
        }

        // --- draw rows ---------------------------------------------------------
        for (i, (spans, alpha)) in rows.iter().skip(self.scroll).take(max_rows).enumerate() {
            let y = base_y - (i as f32 + 1.0) * line_h;
            if y < r.top() {
                break;
            }
            let w = mc.font.spans_width(spans, cs) + 4.0 * cs;
            let bg = Color32::from_black_alpha((128.0 * opacity * alpha) as u8);
            painter.rect_filled(
                Rect::from_min_size(pos2(r.left(), y), vec2(w.min(max_w), line_h)),
                0.0,
                bg,
            );
            // Draw span by span so clickable runs get precise rects.
            let mut x = r.left() + 2.0 * cs;
            for span in spans {
                let mut style = mcui::TextStyle::of_span(span, Color32::WHITE, *alpha);
                // "Chat Colors" off → flatten every run to plain white.
                if !settings.chat_colors {
                    style.color = Color32::from_white_alpha((255.0 * alpha) as u8);
                }
                // "Web Links" off → links are inert (not hoverable/clickable).
                let clickable = span.click.is_some()
                    && (settings.chat_links
                        || !matches!(span.click, Some(ChatClick::OpenUrl(_))));
                let sw = mc.font.width_styled(&span.text, cs, span.bold);
                let rect = Rect::from_min_size(pos2(x, y + 0.5 * cs), vec2(sw, 8.0 * cs));
                let hovered =
                    self.open && clickable && pointer.is_some_and(|p| rect.contains(p));
                if hovered {
                    style.underlined = true;
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    if let Some(tip) = span.hover.as_deref().or(match &span.click {
                        Some(ChatClick::OpenUrl(u)) => Some(u.as_str()),
                        Some(ChatClick::RunCommand(c)) => Some(c.as_str()),
                        _ => None,
                    }) {
                        let tw = mc.font.width(tip, cs) + 6.0 * cs;
                        let tp = pos2(
                            pointer.unwrap().x.min(r.right() - tw),
                            y - LINE_H * cs - 2.0 * cs,
                        );
                        painter.rect_filled(
                            Rect::from_min_size(tp, vec2(tw, LINE_H * cs + 2.0 * cs)),
                            2.0,
                            Color32::from_black_alpha(230),
                        );
                        mc.font.draw(
                            &painter,
                            tp + vec2(3.0 * cs, 1.5 * cs),
                            tip,
                            cs,
                            Color32::from_rgb(0xE0, 0xE0, 0xE0),
                            false,
                        );
                    }
                    if clicked {
                        match span.click.clone().unwrap() {
                            ChatClick::OpenUrl(url) => {
                                if url.starts_with("http://") || url.starts_with("https://") {
                                    let _ = open::that_detached(&url);
                                }
                            }
                            ChatClick::RunCommand(cmd) => {
                                actions.push(HudAction::SendChat(cmd));
                                self.close();
                                actions.push(HudAction::ChatClosed);
                            }
                            ChatClick::SuggestCommand(cmd) => {
                                self.input = cmd;
                                self.cursor = self.input.len();
                                self.sugg = None;
                            }
                            ChatClick::CopyToClipboard(text) => ctx.copy_text(text),
                        }
                    }
                }
                x += mc.font.draw_styled(
                    &painter,
                    pos2(rect.left(), rect.top()),
                    &span.text,
                    cs,
                    style,
                    true,
                    time,
                );
            }
        }
        // Scroll indicator like vanilla's little marker.
        if self.open && self.scroll > 0 {
            mc.font.draw(
                &painter,
                pos2(r.left() + max_w + 4.0 * cs, base_y - line_h),
                &format!("^ {}", self.scroll),
                cs,
                Color32::from_rgb(0xA0, 0xA0, 0xA0),
                true,
            );
        }

        if self.open {
            self.draw_input(ctx, mc, s, cs, &painter, time);
            ctx.request_repaint(); // cursor blink + obfuscated text
        }
        self.just_opened = false;
    }

    /// Keyboard handling while the chat is open. `suggestions` gates the
    /// command auto-complete box (the "Command Suggestions" option).
    fn handle_input(
        &mut self,
        ctx: &egui::Context,
        suggestions: bool,
        actions: &mut Vec<HudAction>,
    ) {
        if self.just_opened {
            // Swallow the keypress that opened the chat (T / slash).
            return;
        }
        let before = self.input.clone();
        mcui::edit_events(ctx, &mut self.input, &mut self.cursor);
        if self.input != before {
            self.history_pos = None;
        }

        let (enter, escape, tab, up, down, page_up, page_down) = ctx.input(|i| {
            (
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::Tab),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::PageUp),
                i.key_pressed(Key::PageDown),
            )
        });

        if enter {
            let msg = std::mem::take(&mut self.input);
            let msg = msg.trim().to_string();
            if !msg.is_empty() {
                if self.history.last() != Some(&msg) {
                    self.history.push(msg.clone());
                }
                actions.push(HudAction::SendChat(msg));
            }
            self.close();
            actions.push(HudAction::ChatClosed);
            return;
        }
        if escape {
            if self.sugg.is_some() {
                self.sugg = None;
            } else {
                self.close();
                actions.push(HudAction::ChatClosed);
            }
            return;
        }

        if page_up {
            self.scroll += ROWS_OPEN - 1;
        }
        if page_down {
            self.scroll = self.scroll.saturating_sub(ROWS_OPEN - 1);
        }

        // Suggestion navigation / application.
        if let Some(sugg) = &mut self.sugg {
            if up {
                sugg.selected = if sugg.selected == 0 {
                    sugg.entries.len() - 1
                } else {
                    sugg.selected - 1
                };
            }
            if down {
                sugg.selected = (sugg.selected + 1) % sugg.entries.len();
            }
            if tab {
                let entry = sugg.entries[sugg.selected].clone();
                let start = sugg.start.min(self.input.len());
                let end = (sugg.start + sugg.length).min(self.input.len());
                self.input.replace_range(start..end, &entry);
                self.cursor = start + entry.len();
                self.sugg = None;
                self.last_requested.clear(); // allow immediate follow-up request
            }
        } else {
            // History browsing (only when no suggestion box is up).
            if up && !self.history.is_empty() {
                match self.history_pos {
                    None => {
                        self.draft = self.input.clone();
                        self.history_pos = Some(self.history.len() - 1);
                    }
                    Some(0) => {}
                    Some(i) => self.history_pos = Some(i - 1),
                }
                if let Some(i) = self.history_pos {
                    self.input = self.history[i].clone();
                    self.cursor = self.input.len();
                }
            }
            if down && self.history_pos.is_some() {
                let i = self.history_pos.unwrap();
                if i + 1 < self.history.len() {
                    self.history_pos = Some(i + 1);
                    self.input = self.history[i + 1].clone();
                } else {
                    self.history_pos = None;
                    self.input = std::mem::take(&mut self.draft);
                }
                self.cursor = self.input.len();
            }
            if tab && self.input.starts_with('/') {
                // No list yet: request one right away.
                self.last_requested.clear();
            }
        }

        // Auto-request completions while typing a command (unless disabled).
        if suggestions && self.input.starts_with('/') && self.input != self.last_requested {
            let id = self.next_req_id;
            self.next_req_id = self.next_req_id.wrapping_add(1).max(1);
            self.pending_req = Some((id, self.input.clone()));
            self.last_requested = self.input.clone();
            actions.push(HudAction::TabComplete { id, text: self.input.clone() });
        }
        if !suggestions || !self.input.starts_with('/') {
            self.sugg = None;
        }
    }

    /// The input bar (and the suggestion box above it).
    fn draw_input(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        cs: f32,
        painter: &egui::Painter,
        time: f64,
    ) {
        let r = ctx.content_rect();
        let h = 12.0 * s;
        let bar = Rect::from_min_size(
            pos2(r.left() + 2.0 * s, r.bottom() - h - 2.0 * s),
            vec2(r.width() - 4.0 * s, h),
        );
        painter.rect_filled(bar, 0.0, Color32::from_black_alpha(128));
        let text_pos = pos2(bar.left() + 2.0 * s, bar.center().y - 4.0 * s);
        mc.font.draw(painter, text_pos, &self.input, s, Color32::WHITE, true);
        // Blinking cursor.
        if (time * 3.0) as u64 % 2 == 0 {
            let cx = text_pos.x + mc.font.width(&self.input[..self.cursor.min(self.input.len())], s);
            painter.rect_filled(
                Rect::from_min_size(pos2(cx, text_pos.y - s), vec2(s.max(1.0), 10.0 * s)),
                0.0,
                Color32::WHITE,
            );
        }

        // Suggestion box, bottom-anchored just above the input bar.
        if let Some(sugg) = &self.sugg {
            let visible = sugg.entries.len().min(10);
            let first = sugg.selected.saturating_sub(visible - 1);
            let width = sugg
                .entries
                .iter()
                .map(|e| mc.font.width(e, cs))
                .fold(0.0f32, f32::max)
                + 6.0 * cs;
            let x = bar.left() + mc.font.width(&self.input[..sugg.start.min(self.input.len())], s);
            let box_h = visible as f32 * LINE_H * cs + 2.0 * cs;
            let top = bar.top() - box_h - 1.0 * s;
            painter.rect_filled(
                Rect::from_min_size(pos2(x, top), vec2(width, box_h)),
                0.0,
                Color32::from_black_alpha(200),
            );
            for (row, i) in (first..sugg.entries.len()).take(visible).enumerate() {
                let color = if i == sugg.selected {
                    Color32::from_rgb(0xFF, 0xFF, 0x00)
                } else {
                    Color32::from_rgb(0xA0, 0xA0, 0xA0)
                };
                mc.font.draw(
                    painter,
                    pos2(x + 3.0 * cs, top + 1.0 * cs + row as f32 * LINE_H * cs),
                    &sugg.entries[i],
                    cs,
                    color,
                    true,
                );
            }
            let _ = time;
        }
    }
}

/// Greedy span-preserving word wrap for the Minecraft font at scale `s`.
/// Returns wrapped lines; every produced span keeps its source style/click.
pub fn wrap_spans(mc: &McUi, spans: &[ChatSpan], s: f32, max_w: f32) -> Vec<Vec<ChatSpan>> {
    let mut lines: Vec<Vec<ChatSpan>> = Vec::new();
    let mut cur: Vec<ChatSpan> = Vec::new();
    let mut cur_w = 0.0f32;

    // (span template, text buffer) — flushed into `cur` when style changes.
    let flush_span = |cur: &mut Vec<ChatSpan>, tpl: &ChatSpan, buf: &mut String| {
        if !buf.is_empty() {
            let mut sp = tpl.clone();
            sp.text = std::mem::take(buf);
            cur.push(sp);
        }
    };

    for span in spans {
        let mut buf = String::new();
        for c in span.text.chars() {
            if c == '\n' {
                flush_span(&mut cur, span, &mut buf);
                lines.push(std::mem::take(&mut cur));
                cur_w = 0.0;
                continue;
            }
            let cw = mc.font.char_advance(c, span.bold) * s;
            if cur_w + cw > max_w && cur_w > 0.0 {
                // Try to break at the last space in the current buffer.
                if let Some(idx) = buf.rfind(' ') {
                    let rest = buf.split_off(idx + 1);
                    flush_span(&mut cur, span, &mut buf);
                    lines.push(std::mem::take(&mut cur));
                    cur_w = mc.font.width_styled(&rest, s, span.bold);
                    buf = rest;
                } else if c == ' ' {
                    flush_span(&mut cur, span, &mut buf);
                    lines.push(std::mem::take(&mut cur));
                    cur_w = 0.0;
                    continue; // swallow the breaking space
                } else {
                    flush_span(&mut cur, span, &mut buf);
                    lines.push(std::mem::take(&mut cur));
                    cur_w = 0.0;
                }
            }
            buf.push(c);
            cur_w += cw;
        }
        flush_span(&mut cur, span, &mut buf);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}
