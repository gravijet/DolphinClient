//! The advancement tree and the screen that shows it.
//!
//! The server sends the whole tree once on join and then a criterion at a time
//! as you earn them. Each advancement knows its parent, where it sits (in cells
//! of 28×27 GUI pixels) and which criteria it needs; an advancement is *done*
//! when every requirement group has at least one criterion stamped with a date
//! — vanilla's AND of ORs.
//!
//! The screen is vanilla's: a window frame with one tab per root advancement,
//! the tab's own tiled background behind the tree, black-and-white lines from
//! each parent to its children, a frame per advancement (square for a task,
//! rounded for a goal, spiky for a challenge, bright once obtained) and a
//! tooltip on hover.

use std::collections::{HashMap, HashSet};

use egui::{Align2, Color32, Rect, Stroke, pos2, vec2};

use crate::app::container;
use crate::app::mcui::McUi;
use crate::assets::items::ItemIcons;
use crate::bridge::events::{AdvancementDisplay, AdvancementUpdate, ChatSpan, ItemSnapshot};

/// Horizontal spacing of one tree cell, in GUI pixels.
const CELL_X: f32 = 28.0;
/// Vertical spacing of one tree cell.
const CELL_Y: f32 = 27.0;
/// The advancement frame sprite is 26×26.
const FRAME: f32 = 26.0;
/// Inner area of the window frame (vanilla's `WINDOW_INSIDE_*`).
const INSIDE_X: f32 = 9.0;
const INSIDE_Y: f32 = 18.0;
const INSIDE_W: f32 = 234.0;
const INSIDE_H: f32 = 113.0;
/// Whole window, including the frame.
pub const WINDOW_W: f32 = 252.0;
pub const WINDOW_H: f32 = 140.0;
/// Tabs are 28×32 and sit on top of the window.
const TAB_W: f32 = 28.0;
const TAB_H: f32 = 32.0;
/// Vanilla fits at most this many tabs across the top.
const MAX_TABS: usize = 7;

/// One advancement, as the tree keeps it.
pub struct Node {
    pub parent: Option<String>,
    pub display: Option<AdvancementDisplay>,
    pub requirements: Vec<Vec<String>>,
    /// Child ids, in the order the server announced them.
    pub children: Vec<String>,
}

/// Everything the client knows about advancements.
#[derive(Default)]
pub struct Advancements {
    pub nodes: HashMap<String, Node>,
    /// Root ids that have a display — one tab each, in announcement order.
    pub roots: Vec<String>,
    /// Obtained criteria per advancement id.
    progress: HashMap<String, HashSet<String>>,
}

impl Advancements {
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.roots.clear();
        self.progress.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Apply one `ClientboundUpdateAdvancements`. Returns the ids that went
    /// from "not done" to "done" *and* want a toast, so the caller can pop one.
    pub fn apply(&mut self, update: &AdvancementUpdate) -> Vec<String> {
        if update.reset {
            self.clear();
        }
        for id in &update.removed {
            if let Some(node) = self.nodes.remove(id)
                && let Some(parent) = node.parent.as_ref()
                && let Some(p) = self.nodes.get_mut(parent)
            {
                p.children.retain(|c| c != id);
            }
            self.roots.retain(|r| r != id);
            self.progress.remove(id);
        }
        for node in &update.added {
            let has_display = node.display.is_some();
            self.nodes.insert(node.id.clone(), Node {
                parent: node.parent.clone(),
                display: node.display.clone(),
                requirements: node.requirements.clone(),
                children: Vec::new(),
            });
            match &node.parent {
                Some(parent) => {
                    if let Some(p) = self.nodes.get_mut(parent)
                        && !p.children.contains(&node.id)
                    {
                        p.children.push(node.id.clone());
                    }
                }
                None => {
                    // Only advancements with a display get a tab; the rest are
                    // the invisible "glue" servers use for logic.
                    if has_display && !self.roots.contains(&node.id) {
                        self.roots.push(node.id.clone());
                    }
                }
            }
        }
        // A child can arrive before its parent, so re-link anything orphaned.
        let links: Vec<(String, String)> = self
            .nodes
            .iter()
            .filter_map(|(id, n)| n.parent.clone().map(|p| (p, id.clone())))
            .collect();
        for (parent, child) in links {
            if let Some(p) = self.nodes.get_mut(&parent)
                && !p.children.contains(&child)
            {
                p.children.push(child);
            }
        }
        let mut newly_done = Vec::new();
        for (id, obtained) in &update.progress {
            let was_done = self.is_done(id);
            self.progress.insert(id.clone(), obtained.iter().cloned().collect());
            if !was_done && self.is_done(id) && !update.reset {
                newly_done.push(id.clone());
            }
        }
        newly_done
    }

    /// Vanilla's rule: every requirement group needs one obtained criterion,
    /// and an advancement with no requirements is never done.
    pub fn is_done(&self, id: &str) -> bool {
        let Some(node) = self.nodes.get(id) else { return false };
        if node.requirements.is_empty() {
            return false;
        }
        let empty = HashSet::new();
        let got = self.progress.get(id).unwrap_or(&empty);
        node.requirements
            .iter()
            .all(|group| group.iter().any(|c| got.contains(c)))
    }

    /// `(obtained groups, total groups)` — vanilla's "3/5" tooltip counter.
    pub fn progress_of(&self, id: &str) -> (usize, usize) {
        let Some(node) = self.nodes.get(id) else { return (0, 0) };
        let empty = HashSet::new();
        let got = self.progress.get(id).unwrap_or(&empty);
        let done = node
            .requirements
            .iter()
            .filter(|group| group.iter().any(|c| got.contains(c)))
            .count();
        (done, node.requirements.len())
    }

    /// How many advancements under this root are done, out of how many are
    /// visible — the number the tab tooltip shows.
    pub fn tab_progress(&self, root: &str) -> (usize, usize) {
        let mut done = 0;
        let mut total = 0;
        let mut stack = vec![root.to_string()];
        while let Some(id) = stack.pop() {
            let Some(node) = self.nodes.get(&id) else { continue };
            if node.display.is_some() {
                total += 1;
                if self.is_done(&id) {
                    done += 1;
                }
            }
            stack.extend(node.children.iter().cloned());
        }
        (done, total)
    }

    /// Every displayed advancement under `root`, with its position in cells.
    fn tree_of<'a>(&'a self, root: &str) -> Vec<(&'a str, &'a AdvancementDisplay)> {
        let mut out = Vec::new();
        // Walk from the map's own key so every id borrowed below outlives the
        // call, children included.
        let mut stack: Vec<&'a str> = match self.nodes.get_key_value(root) {
            Some((key, _)) => vec![key.as_str()],
            None => Vec::new(),
        };
        while let Some(id) = stack.pop() {
            let Some(node) = self.nodes.get(id) else { continue };
            if let Some(display) = &node.display
                && !display.hidden
            {
                out.push((id, display));
            }
            for child in &node.children {
                stack.push(child.as_str());
            }
        }
        out
    }
}

/// Where the screen is scrolled to and which tab is open — kept across frames.
#[derive(Default)]
pub struct AdvancementsView {
    pub tab: usize,
    pub scroll: (f32, f32),
    /// Set once the first frame has centred the tree on its root.
    centred: bool,
}

impl AdvancementsView {
    pub fn reset(&mut self) {
        self.tab = 0;
        self.scroll = (0.0, 0.0);
        self.centred = false;
    }
}

/// Draw vanilla's advancements screen. Returns `true` when it drew a real
/// tree (so the caller can skip its "nothing here yet" placeholder).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ctx: &egui::Context,
    mc: &McUi,
    s: f32,
    tree: &Advancements,
    view: &mut AdvancementsView,
    icons: &Option<(egui::TextureId, std::sync::Arc<ItemIcons>)>,
    lang: &crate::assets::Lang,
) -> bool {
    if tree.roots.is_empty() {
        return false;
    }
    view.tab = view.tab.min(tree.roots.len().saturating_sub(1));
    let root = tree.roots[view.tab].clone();

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("advancements"),
    ));
    let screen = ctx.content_rect();
    let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    let win = Rect::from_center_size(screen.center(), vec2(WINDOW_W * s, WINDOW_H * s));
    let inside = Rect::from_min_size(
        win.min + vec2(INSIDE_X * s, INSIDE_Y * s),
        vec2(INSIDE_W * s, INSIDE_H * s),
    );

    let items = tree.tree_of(&root);
    // Centre the tree the first time this tab is opened, exactly like vanilla
    // does when the screen opens.
    if !view.centred {
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
        let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
        for (_, d) in &items {
            min_x = min_x.min(d.x);
            max_x = max_x.max(d.x);
            min_y = min_y.min(d.y);
            max_y = max_y.max(d.y);
        }
        if min_x <= max_x {
            view.scroll = (
                -(min_x + max_x) / 2.0 * CELL_X + INSIDE_W / 2.0 - FRAME / 2.0,
                -(min_y + max_y) / 2.0 * CELL_Y + INSIDE_H / 2.0 - FRAME / 2.0,
            );
        }
        view.centred = true;
    }

    // Drag to pan, exactly like vanilla's click-and-drag tree.
    let (down, delta, over) = ctx.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.delta(),
            i.pointer.latest_pos().is_some_and(|p| inside.contains(p)),
        )
    });
    if down && over {
        view.scroll.0 += delta.x / s;
        view.scroll.1 += delta.y / s;
    }

    // Background: the tab's own texture, tiled 16×16 over the inside area and
    // clipped to it, then dimmed the way vanilla dims it.
    let bg_name = tree
        .nodes
        .get(&root)
        .and_then(|n| n.display.as_ref())
        .and_then(|d| d.background.as_deref())
        .map(background_key)
        .unwrap_or("stone");
    let clipped = painter.with_clip_rect(inside);
    if let Some(tex) = mc.tex.advancement_bg.get(bg_name).or_else(|| mc.tex.advancement_bg.get("stone")) {
        let tile = 16.0 * s;
        // Tiles scroll with the tree so panning reads as movement.
        let ox = (view.scroll.0 * s).rem_euclid(tile) - tile;
        let oy = (view.scroll.1 * s).rem_euclid(tile) - tile;
        let mut y = inside.top() + oy;
        while y < inside.bottom() {
            let mut x = inside.left() + ox;
            while x < inside.right() {
                clipped.image(
                    tex.id(),
                    Rect::from_min_size(pos2(x, y), vec2(tile, tile)),
                    full,
                    Color32::from_gray(120),
                );
                x += tile;
            }
            y += tile;
        }
    } else {
        clipped.rect_filled(inside, 0.0, Color32::from_rgb(20, 20, 20));
    }

    // Where an advancement's frame lands on screen.
    let place = |d: &AdvancementDisplay| {
        pos2(
            inside.left() + (d.x * CELL_X + view.scroll.0) * s,
            inside.top() + (d.y * CELL_Y + view.scroll.1) * s,
        )
    };

    // Connecting lines: vanilla draws a thick black elbow and a thin white one
    // over it, from the middle of the parent to the middle of the child.
    for (id, display) in &items {
        let Some(node) = tree.nodes.get(*id) else { continue };
        let Some(parent) = node.parent.as_ref() else { continue };
        let Some(pdisplay) = tree.nodes.get(parent).and_then(|p| p.display.as_ref()) else {
            continue;
        };
        let a = place(pdisplay) + vec2(FRAME * s / 2.0, FRAME * s / 2.0);
        let b = place(display) + vec2(FRAME * s / 2.0, FRAME * s / 2.0);
        // Vanilla bends the line four pixels past the parent's right edge, so
        // it runs between the frames instead of across them.
        let bend = place(pdisplay).x + (FRAME + 4.0) * s;
        let elbow = [pos2(a.x, a.y), pos2(bend, a.y), pos2(bend, b.y), pos2(b.x, b.y)];
        for (width, color) in [(3.0 * s, Color32::BLACK), (s, Color32::WHITE)] {
            for pair in elbow.windows(2) {
                clipped.line_segment([pair[0], pair[1]], Stroke::new(width, color));
            }
        }
    }

    // Frames + icons.
    let pointer = ctx.pointer_hover_pos();
    let mut hovered: Option<(&str, &AdvancementDisplay)> = None;
    for (id, display) in &items {
        let at = place(display);
        let rect = Rect::from_min_size(at, vec2(FRAME * s, FRAME * s));
        if !inside.intersects(rect) {
            continue;
        }
        let done = tree.is_done(id);
        let sprite = format!(
            "{}_frame_{}",
            match display.frame {
                1 => "challenge",
                2 => "goal",
                _ => "task",
            },
            if done { "obtained" } else { "unobtained" }
        );
        if let Some(tex) = mc.tex.advancement.get(sprite.as_str()) {
            clipped.image(tex.id(), rect, full, Color32::WHITE);
        }
        if let Some(icon) = &display.icon {
            let cell = Rect::from_min_size(at + vec2(5.0 * s, 5.0 * s), vec2(16.0 * s, 16.0 * s));
            container::draw_item(&clipped, mc, icons, cell, icon, s);
        }
        if pointer.is_some_and(|p| rect.contains(p) && inside.contains(p)) {
            hovered = Some((id, display));
        }
    }

    // Window frame on top of the tree (its hole is exactly the inside area).
    if let Some(tex) = &mc.tex.advancement_window {
        // The frame lives in the top-left 252×140 of a 256×256 sheet.
        painter.image(
            tex.id(),
            win,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(WINDOW_W / 256.0, WINDOW_H / 256.0)),
            Color32::WHITE,
        );
    }

    // Tabs across the top, one per root.
    let mut clicked_tab = None;
    for (i, id) in tree.roots.iter().take(MAX_TABS).enumerate() {
        let selected = i == view.tab;
        let kind = match i {
            0 => "left",
            n if n == tree.roots.len().min(MAX_TABS) - 1 => "right",
            _ => "middle",
        };
        let name = if selected {
            format!("tab_above_{kind}_selected")
        } else {
            format!("tab_above_{kind}")
        };
        let rect = Rect::from_min_size(
            pos2(win.left() + i as f32 * TAB_W * s, win.top() - (TAB_H - 4.0) * s),
            vec2(TAB_W * s, TAB_H * s),
        );
        if let Some(tex) = mc.tex.advancement.get(name.as_str()) {
            painter.image(tex.id(), rect, full, Color32::WHITE);
        }
        if let Some(icon) = tree.nodes.get(id).and_then(|n| n.display.as_ref()).and_then(|d| d.icon.as_ref()) {
            let cell = Rect::from_min_size(
                rect.min + vec2(6.0 * s, if selected { 5.0 } else { 7.0 } * s),
                vec2(16.0 * s, 16.0 * s),
            );
            container::draw_item(&painter, mc, icons, cell, icon, s);
        }
        if pointer.is_some_and(|p| rect.contains(p)) && ctx.input(|i| i.pointer.primary_pressed()) {
            clicked_tab = Some(i);
        }
    }
    if let Some(i) = clicked_tab {
        view.tab = i;
        view.centred = false;
    }

    // Title: the open tab's own name, in the window's title bar.
    if let Some(display) = tree.nodes.get(&root).and_then(|n| n.display.as_ref()) {
        mc.font.draw_spans_anchored(
            &painter,
            win.min + vec2(8.0 * s, 6.0 * s),
            Align2::LEFT_TOP,
            &display.title,
            s,
            Color32::from_rgb(0x40, 0x40, 0x40),
            false,
            0.0,
        );
    }

    // Hover tooltip: title, description, and the criteria counter when there
    // is more than one requirement group.
    if let Some((id, display)) = hovered {
        let (got, total) = tree.progress_of(id);
        let mut lines: Vec<Vec<ChatSpan>> = Vec::new();
        let mut header = display.title.clone();
        if total > 1 {
            header.push(ChatSpan {
                text: format!("  {got}/{total}"),
                color: Some([0xAA, 0xAA, 0xAA]),
                ..Default::default()
            });
        }
        lines.push(header);
        if !display.description.is_empty() {
            lines.push(display.description.clone());
        }
        if !tree.is_done(id) {
            lines.push(vec![ChatSpan {
                text: lang.get("advancements.sad_label").unwrap_or(":(").to_string(),
                color: Some([0xAA, 0xAA, 0xAA]),
                ..Default::default()
            }]);
        }
        let at = place(display) + vec2(FRAME * s, 0.0);
        draw_tooltip(&painter, mc, s, at, &lines, screen);
    }
    true
}

/// A vanilla-styled dark tooltip box with a violet border.
fn draw_tooltip(
    painter: &egui::Painter,
    mc: &McUi,
    s: f32,
    at: egui::Pos2,
    lines: &[Vec<ChatSpan>],
    screen: Rect,
) {
    let width = lines.iter().map(|l| mc.font.spans_width(l, s)).fold(0.0, f32::max);
    let height = lines.len() as f32 * 10.0 * s;
    let mut min = at;
    if min.x + width + 8.0 * s > screen.right() {
        min.x = screen.right() - width - 8.0 * s;
    }
    if min.y + height + 8.0 * s > screen.bottom() {
        min.y = screen.bottom() - height - 8.0 * s;
    }
    let rect = Rect::from_min_size(min, vec2(width + 8.0 * s, height + 6.0 * s));
    painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(16, 0, 16, 240));
    painter.rect_stroke(
        rect.shrink(s),
        0.0,
        Stroke::new(s, Color32::from_rgb(0x50, 0x28, 0xA0)),
        egui::StrokeKind::Inside,
    );
    for (i, line) in lines.iter().enumerate() {
        mc.font.draw_spans_anchored(
            painter,
            rect.min + vec2(4.0 * s, (4.0 + i as f32 * 10.0) * s),
            Align2::LEFT_TOP,
            line,
            s,
            Color32::WHITE,
            true,
            0.0,
        );
    }
}

/// `minecraft:textures/gui/advancements/backgrounds/nether.png` → `nether`.
fn background_key(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path).trim_end_matches(".png")
}

/// The item shown in an advancement toast/frame, or a stone stand-in.
pub fn icon_or_stone(display: &AdvancementDisplay) -> Option<ItemSnapshot> {
    display.icon.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::events::AdvancementNode;

    fn display(title: &str) -> AdvancementDisplay {
        AdvancementDisplay {
            title: vec![ChatSpan::plain(title)],
            description: vec![ChatSpan::plain("desc")],
            icon: None,
            frame: 0,
            show_toast: true,
            hidden: false,
            background: Some("minecraft:textures/gui/advancements/backgrounds/stone.png".into()),
            x: 0.0,
            y: 0.0,
        }
    }

    fn node(id: &str, parent: Option<&str>, reqs: Vec<Vec<&str>>) -> AdvancementNode {
        AdvancementNode {
            id: id.to_string(),
            parent: parent.map(str::to_string),
            display: Some(display(id)),
            requirements: reqs
                .into_iter()
                .map(|g| g.into_iter().map(str::to_string).collect())
                .collect(),
        }
    }

    #[test]
    fn roots_become_tabs_and_children_link_up() {
        let mut tree = Advancements::default();
        tree.apply(&AdvancementUpdate {
            added: vec![
                node("story/root", None, vec![vec!["a"]]),
                node("story/stone", Some("story/root"), vec![vec!["b"]]),
            ],
            ..Default::default()
        });
        assert_eq!(tree.roots, vec!["story/root".to_string()]);
        assert_eq!(tree.nodes["story/root"].children, vec!["story/stone".to_string()]);
    }

    #[test]
    fn a_child_announced_before_its_parent_still_links() {
        let mut tree = Advancements::default();
        tree.apply(&AdvancementUpdate {
            added: vec![
                node("story/stone", Some("story/root"), vec![vec!["b"]]),
                node("story/root", None, vec![vec!["a"]]),
            ],
            ..Default::default()
        });
        assert_eq!(tree.nodes["story/root"].children, vec!["story/stone".to_string()]);
    }

    #[test]
    fn every_group_needs_one_criterion() {
        let mut tree = Advancements::default();
        tree.apply(&AdvancementUpdate {
            added: vec![node("t", None, vec![vec!["a", "b"], vec!["c"]])],
            progress: vec![("t".into(), vec!["a".into()])],
            ..Default::default()
        });
        assert!(!tree.is_done("t"), "one group still unsatisfied");
        assert_eq!(tree.progress_of("t"), (1, 2));
        tree.apply(&AdvancementUpdate {
            progress: vec![("t".into(), vec!["a".into(), "c".into()])],
            ..Default::default()
        });
        assert!(tree.is_done("t"));
    }

    #[test]
    fn an_advancement_with_no_requirements_is_never_done() {
        let mut tree = Advancements::default();
        tree.apply(&AdvancementUpdate {
            added: vec![node("t", None, vec![])],
            ..Default::default()
        });
        assert!(!tree.is_done("t"));
    }

    #[test]
    fn completing_one_reports_it_exactly_once() {
        let mut tree = Advancements::default();
        tree.apply(&AdvancementUpdate {
            added: vec![node("t", None, vec![vec!["a"]])],
            ..Default::default()
        });
        let first = tree.apply(&AdvancementUpdate {
            progress: vec![("t".into(), vec!["a".into()])],
            ..Default::default()
        });
        assert_eq!(first, vec!["t".to_string()]);
        let again = tree.apply(&AdvancementUpdate {
            progress: vec![("t".into(), vec!["a".into()])],
            ..Default::default()
        });
        assert!(again.is_empty(), "already-done advancements must not re-toast");
    }

    #[test]
    fn the_join_dump_does_not_toast_everything_you_ever_did() {
        let mut tree = Advancements::default();
        let done = tree.apply(&AdvancementUpdate {
            reset: true,
            added: vec![node("t", None, vec![vec!["a"]])],
            progress: vec![("t".into(), vec!["a".into()])],
            ..Default::default()
        });
        assert!(done.is_empty());
        assert!(tree.is_done("t"));
    }

    #[test]
    fn tab_progress_counts_the_whole_branch() {
        let mut tree = Advancements::default();
        tree.apply(&AdvancementUpdate {
            added: vec![
                node("root", None, vec![vec!["a"]]),
                node("kid", Some("root"), vec![vec!["b"]]),
                node("grandkid", Some("kid"), vec![vec!["c"]]),
            ],
            progress: vec![("root".into(), vec!["a".into()])],
            ..Default::default()
        });
        assert_eq!(tree.tab_progress("root"), (1, 3));
    }

    #[test]
    fn background_names_come_out_of_the_asset_path() {
        assert_eq!(
            background_key("minecraft:textures/gui/advancements/backgrounds/nether.png"),
            "nether"
        );
        assert_eq!(background_key("stone"), "stone");
    }
}
