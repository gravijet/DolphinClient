//! Convert azalea chat components (`FormattedText`) into plain, styled
//! [`ChatSpan`]s the UI can draw. Handles both the modern component styles
//! (color / bold / click events / …) and the legacy `§x` codes many servers
//! still embed inside text nodes.

use azalea_chat::FormattedText;
use azalea_chat::click_event::ClickEvent;
use azalea_chat::hover_event::HoverEvent;
use azalea_chat::style::Style;
use azalea_chat::text_component::TextComponent;

use super::events::{ChatClick, ChatSpan};

/// The 16 legacy `§0`-`§f` colors (vanilla RGB values).
pub const LEGACY_COLORS: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00], // 0 black
    [0x00, 0x00, 0xAA], // 1 dark_blue
    [0x00, 0xAA, 0x00], // 2 dark_green
    [0x00, 0xAA, 0xAA], // 3 dark_aqua
    [0xAA, 0x00, 0x00], // 4 dark_red
    [0xAA, 0x00, 0xAA], // 5 dark_purple
    [0xFF, 0xAA, 0x00], // 6 gold
    [0xAA, 0xAA, 0xAA], // 7 gray
    [0x55, 0x55, 0x55], // 8 dark_gray
    [0x55, 0x55, 0xFF], // 9 blue
    [0x55, 0xFF, 0x55], // a green
    [0x55, 0xFF, 0xFF], // b aqua
    [0xFF, 0x55, 0x55], // c red
    [0xFF, 0x55, 0xFF], // d light_purple
    [0xFF, 0xFF, 0x55], // e yellow
    [0xFF, 0xFF, 0xFF], // f white
];

/// Effective (inherited) style, flattened to plain data.
#[derive(Clone, Copy, Default)]
struct Flat {
    color: Option<[u8; 3]>,
    bold: bool,
    italic: bool,
    underlined: bool,
    strikethrough: bool,
    obfuscated: bool,
}

impl Flat {
    fn apply(mut self, s: &Style) -> Flat {
        if let Some(c) = &s.color {
            self.color = Some(rgb(c.value));
        }
        if let Some(b) = s.bold {
            self.bold = b;
        }
        if let Some(b) = s.italic {
            self.italic = b;
        }
        if let Some(b) = s.underlined {
            self.underlined = b;
        }
        if let Some(b) = s.strikethrough {
            self.strikethrough = b;
        }
        if let Some(b) = s.obfuscated {
            self.obfuscated = b;
        }
        self
    }
}

fn rgb(value: u32) -> [u8; 3] {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

fn click_of(s: &Style) -> Option<ChatClick> {
    match s.click_event.as_ref()? {
        ClickEvent::OpenUrl { url } => Some(ChatClick::OpenUrl(url.clone())),
        ClickEvent::RunCommand { command } => Some(ChatClick::RunCommand(command.clone())),
        ClickEvent::SuggestCommand { command } => Some(ChatClick::SuggestCommand(command.clone())),
        ClickEvent::CopyToClipboard { value } => Some(ChatClick::CopyToClipboard(value.clone())),
        _ => None,
    }
}

fn hover_of(s: &Style) -> Option<String> {
    match s.hover_event.as_ref()? {
        HoverEvent::ShowText { value } => {
            let text = plain_text(value);
            (!text.trim().is_empty()).then_some(text)
        }
        _ => None,
    }
}

/// Flatten a component tree to unstyled text (tooltips, logging).
pub fn plain_text(ft: &FormattedText) -> String {
    super::events::spans_to_plain(&spans_of(ft))
}

/// Convert a whole component tree into styled spans.
pub fn spans_of(ft: &FormattedText) -> Vec<ChatSpan> {
    let mut out = Vec::new();
    walk(ft, Flat::default(), None, None, &mut out);
    // Drop empty spans; keep at least one so callers always see a line.
    out.retain(|s| !s.text.is_empty());
    if out.is_empty() {
        out.push(ChatSpan::plain(""));
    }
    out
}

fn walk(
    ft: &FormattedText,
    parent: Flat,
    parent_click: Option<ChatClick>,
    parent_hover: Option<String>,
    out: &mut Vec<ChatSpan>,
) {
    match ft {
        FormattedText::Text(tc) => {
            walk_text(tc, parent, parent_click, parent_hover, out);
        }
        FormattedText::Translatable(tr) => {
            let style = &tr.base.style;
            let flat = parent.apply(style);
            let click = click_of(style).or(parent_click);
            let hover = hover_of(style).or(parent_hover);
            // read() resolves the translation key + inlines the args (their
            // own styles survive as nested components).
            if let Ok(tc) = tr.read() {
                walk_text(&tc, flat, click.clone(), hover.clone(), out);
            }
            for sib in &tr.base.siblings {
                walk(sib, flat, click.clone(), hover.clone(), out);
            }
        }
    }
}

fn walk_text(
    tc: &TextComponent,
    parent: Flat,
    parent_click: Option<ChatClick>,
    parent_hover: Option<String>,
    out: &mut Vec<ChatSpan>,
) {
    let style = &tc.base.style;
    let flat = parent.apply(style);
    let click = click_of(style).or(parent_click);
    let hover = hover_of(style).or(parent_hover);
    push_legacy(&tc.text, flat, click.clone(), hover.clone(), out);
    for sib in &tc.base.siblings {
        walk(sib, flat, click.clone(), hover.clone(), out);
    }
}

/// Append `text` as spans, splitting on embedded legacy `§x` codes.
fn push_legacy(
    text: &str,
    base: Flat,
    click: Option<ChatClick>,
    hover: Option<String>,
    out: &mut Vec<ChatSpan>,
) {
    let mut cur = base;
    let mut buf = String::new();
    let mut chars = text.chars();
    let flush = |buf: &mut String, cur: Flat, out: &mut Vec<ChatSpan>| {
        if !buf.is_empty() {
            out.push(ChatSpan {
                text: std::mem::take(buf),
                color: cur.color,
                bold: cur.bold,
                italic: cur.italic,
                underlined: cur.underlined,
                strikethrough: cur.strikethrough,
                obfuscated: cur.obfuscated,
                click: click.clone(),
                hover: hover.clone(),
            });
        }
    };
    while let Some(c) = chars.next() {
        if c != '§' {
            buf.push(c);
            continue;
        }
        let Some(code) = chars.next() else { break };
        flush(&mut buf, cur, out);
        match code.to_ascii_lowercase() {
            c @ '0'..='9' => {
                cur = Flat { color: Some(LEGACY_COLORS[c as usize - '0' as usize]), ..Flat::default() };
            }
            c @ 'a'..='f' => {
                cur = Flat { color: Some(LEGACY_COLORS[c as usize - 'a' as usize + 10]), ..Flat::default() };
            }
            'k' => cur.obfuscated = true,
            'l' => cur.bold = true,
            'm' => cur.strikethrough = true,
            'n' => cur.underlined = true,
            'o' => cur.italic = true,
            'r' => cur = base,
            _ => {} // unknown code: swallow, like vanilla
        }
    }
    flush(&mut buf, cur, out);
}

/// Parse a standalone legacy string (no component tree) into spans — used for
/// server-list MOTDs that arrive as plain `§`-coded strings.
pub fn spans_of_legacy(text: &str) -> Vec<ChatSpan> {
    let mut out = Vec::new();
    push_legacy(text, Flat::default(), None, None, &mut out);
    out.retain(|s| !s.text.is_empty());
    if out.is_empty() {
        out.push(ChatSpan::plain(""));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use azalea_chat::text_component::TextComponent;

    #[test]
    fn legacy_codes_become_colored_spans() {
        // Vanilla order: color first, then formatting (§c§l = bold red).
        let spans = spans_of_legacy("§aHello §c§lWorld§r!");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "Hello ");
        assert_eq!(spans[0].color, Some([0x55, 0xFF, 0x55]));
        assert_eq!(spans[1].text, "World");
        assert_eq!(spans[1].color, Some([0xFF, 0x55, 0x55]));
        assert!(spans[1].bold);
        assert_eq!(spans[2].text, "!");
        assert_eq!(spans[2].color, None);
        assert!(!spans[2].bold);
    }

    #[test]
    fn component_styles_inherit() {
        let mut root = TextComponent::new("A");
        root.base.style.bold = Some(true);
        let mut child = TextComponent::new("B");
        child.base.style.italic = Some(true);
        root.base.siblings.push(FormattedText::Text(child));
        let spans = spans_of(&FormattedText::Text(root));
        assert_eq!(spans.len(), 2);
        assert!(spans[0].bold && !spans[0].italic);
        assert!(spans[1].bold && spans[1].italic, "sibling inherits bold");
    }

    #[test]
    fn color_code_resets_formatting() {
        let spans = spans_of_legacy("§6§lGold§7Gray");
        assert_eq!(spans[0].text, "Gold");
        assert!(spans[0].bold, "formatting after the color code sticks");
        assert_eq!(spans[1].text, "Gray");
        assert!(!spans[1].bold, "color code resets bold (vanilla semantics)");
    }

    #[test]
    fn plain_flattening() {
        let spans = spans_of_legacy("§ahi §cthere");
        assert_eq!(crate::bridge::events::spans_to_plain(&spans), "hi there");
    }
}
