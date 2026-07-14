//! Custom typography for the launcher.
//!
//! A generic egui launcher instantly reads as "a Rust app" because it ships
//! egui's default proportional face. We replace it with a deliberate pairing:
//! **Outfit** (a clean geometric grotesque, SIL OFL) for all UI text in a few
//! weights, and **Sora** (SIL OFL) ExtraBold for big display headings and the
//! LAUNCH button. The vanilla egui fonts stay as glyph fallbacks (emoji, rare
//! symbols).
//!
//! Weights are exposed as named font families so callers can pick one with
//! `FontId::new(size, FontFamily::Name("semibold".into()))`.

use eframe::egui::{self, FontData, FontFamily, FontDefinitions};

const OUTFIT_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Outfit-Regular.ttf");
const OUTFIT_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Outfit-Medium.ttf");
const OUTFIT_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Outfit-SemiBold.ttf");
const OUTFIT_BOLD: &[u8] = include_bytes!("../../assets/fonts/Outfit-Bold.ttf");
const SORA_EXTRABOLD: &[u8] = include_bytes!("../../assets/fonts/Sora-ExtraBold.ttf");

/// Family name for medium-weight UI text.
pub const MEDIUM: &str = "medium";
/// Family name for semibold UI text (labels, buttons, nav).
pub const SEMIBOLD: &str = "semibold";
/// Family name for bold UI text.
pub const BOLD: &str = "bold";
/// Family name for the big display face (Sora ExtraBold).
pub const DISPLAY: &str = "display";

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert("Outfit".to_owned(), FontData::from_static(OUTFIT_REGULAR));
    fonts.font_data.insert("Outfit-Medium".to_owned(), FontData::from_static(OUTFIT_MEDIUM));
    fonts.font_data.insert("Outfit-SemiBold".to_owned(), FontData::from_static(OUTFIT_SEMIBOLD));
    fonts.font_data.insert("Outfit-Bold".to_owned(), FontData::from_static(OUTFIT_BOLD));
    fonts.font_data.insert("Sora-ExtraBold".to_owned(), FontData::from_static(SORA_EXTRABOLD));

    // Keep egui's default proportional stack as glyph fallback (emoji, symbols
    // Outfit/Sora don't cover), but put Outfit first so normal text uses it.
    let fallback: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let with = |first: &str| {
        let mut v = vec![first.to_owned()];
        v.extend(fallback.iter().cloned());
        v
    };

    fonts.families.insert(FontFamily::Proportional, with("Outfit"));
    fonts.families.insert(FontFamily::Name(MEDIUM.into()), with("Outfit-Medium"));
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), with("Outfit-SemiBold"));
    fonts.families.insert(FontFamily::Name(BOLD.into()), with("Outfit-Bold"));
    fonts.families.insert(FontFamily::Name(DISPLAY.into()), with("Sora-ExtraBold"));

    ctx.set_fonts(fonts);
}
