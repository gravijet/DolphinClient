//! The player tab list (hold Tab): header/footer, one row per player with
//! skin head, styled display name and vanilla ping bars.

use egui::{Color32, Id, LayerId, Order, Rect, pos2, vec2};

use crate::app::mcui::{LINE_H, McUi};
use crate::app::skins::SkinManager;
use crate::bridge::events::{ChatSpan, TabPlayer};

/// Ping bars sprite index for a latency (matches vanilla thresholds).
fn ping_index(latency: i32) -> usize {
    match latency {
        l if l < 0 => 5, // unknown
        0..150 => 4,     // 5 bars
        150..300 => 3,
        300..600 => 2,
        600..1000 => 1,
        _ => 0,
    }
}

pub struct TabListState {
    pub players: Vec<TabPlayer>,
    pub header: Vec<ChatSpan>,
    pub footer: Vec<ChatSpan>,
}

impl Default for TabListState {
    fn default() -> Self {
        Self { players: Vec::new(), header: Vec::new(), footer: Vec::new() }
    }
}

/// Draw the overlay (call while the player-list key is held).
pub fn draw(
    ctx: &egui::Context,
    mc: &McUi,
    s: f32,
    state: &TabListState,
    skins: &mut SkinManager,
) {
    if state.players.is_empty() {
        return;
    }
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("tab-list")));
    let screen = ctx.content_rect();
    let time = ctx.input(|i| i.time);

    // Vanilla: up to 20 rows per column.
    let n = state.players.len();
    let cols = n.div_ceil(20).max(1);
    let rows = n.div_ceil(cols);

    // Row metrics (GUI px): head 8 + gap + name + ping icon 10.
    let name_w = state
        .players
        .iter()
        .map(|p| mc.font.spans_width(&p.display, s))
        .fold(0.0f32, f32::max)
        .max(40.0 * s);
    let row_w = 2.0 * s + 8.0 * s + 2.0 * s + name_w + 4.0 * s + 10.0 * s + 2.0 * s;
    let row_h = LINE_H * s;
    let grid_w = cols as f32 * row_w + (cols as f32 - 1.0) * 2.0 * s;

    let header_lines: usize = if state.header.is_empty() { 0 } else { 1 };
    let footer_lines: usize = if state.footer.is_empty() { 0 } else { 1 };
    let total_h = (rows + header_lines + footer_lines) as f32 * row_h + 8.0 * s;
    let top = screen.top() + 10.0 * s;
    let left = screen.center().x - grid_w / 2.0;

    // Background.
    painter.rect_filled(
        Rect::from_min_size(
            pos2(left - 2.0 * s, top - 2.0 * s),
            vec2(grid_w + 4.0 * s, total_h + 4.0 * s),
        ),
        0.0,
        Color32::from_black_alpha(120),
    );

    let mut y = top;
    if header_lines > 0 {
        mc.font.draw_spans_anchored(
            &painter,
            pos2(screen.center().x, y + 4.0 * s),
            egui::Align2::CENTER_CENTER,
            &state.header,
            s,
            Color32::WHITE,
            true,
            time,
        );
        y += row_h + 2.0 * s;
    }

    for (i, p) in state.players.iter().enumerate() {
        let col = i / rows;
        let row = i % rows;
        let x = left + col as f32 * (row_w + 2.0 * s);
        let ry = y + row as f32 * row_h;
        let r = Rect::from_min_size(pos2(x, ry), vec2(row_w, row_h - 1.0));
        painter.rect_filled(r, 0.0, Color32::from_white_alpha(20));

        // Head (skin face) — requested lazily, drawn once downloaded.
        let head_rect =
            Rect::from_min_size(pos2(x + 1.0 * s, ry + 0.5 * s), vec2(8.0 * s, 8.0 * s));
        let mut drew_head = false;
        if let Some(url) = &p.skin_url {
            skins.request(url);
            if let Some(tex) = skins.head(ctx, url) {
                painter.image(
                    tex.id(),
                    head_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                drew_head = true;
            }
        }
        if !drew_head {
            painter.rect_filled(head_rect, 0.0, Color32::from_gray(60));
        }

        mc.font.draw_spans(
            &painter,
            pos2(x + 12.0 * s, ry + 0.5 * s),
            &p.display,
            s,
            Color32::WHITE,
            1.0,
            true,
            time,
        );

        // Ping bars, right-aligned.
        let icon = &mc.tex.ping[ping_index(p.latency)];
        let icon_size = vec2(10.0 * s, 7.0 * s);
        painter.image(
            icon.id(),
            Rect::from_min_size(pos2(x + row_w - 11.0 * s, ry + 0.5 * s), icon_size),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    if footer_lines > 0 {
        mc.font.draw_spans_anchored(
            &painter,
            pos2(screen.center().x, y + rows as f32 * row_h + 4.0 * s),
            egui::Align2::CENTER_CENTER,
            &state.footer,
            s,
            Color32::WHITE,
            true,
            time,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_bars_follow_vanilla_thresholds() {
        assert_eq!(ping_index(-1), 5);
        assert_eq!(ping_index(30), 4);
        assert_eq!(ping_index(200), 3);
        assert_eq!(ping_index(450), 2);
        assert_eq!(ping_index(800), 1);
        assert_eq!(ping_index(2000), 0);
    }
}
