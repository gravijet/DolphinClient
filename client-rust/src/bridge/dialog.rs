//! Parses vanilla's `ClientboundShowDialog` payload — a raw, server-authored
//! NBT document azalea does not know the schema of at all (`Holder<Dialog,
//! Nbt>`) — into the phase-1 subset of the real "Dialogs" system this client
//! renders: `notice`/`confirmation` dialogs with `plain_message` bodies and a
//! bounded [`ButtonAction`] set. Every field name, default and dispatch key
//! below is transcribed from the decompiled 26.1 client jar's
//! `net.minecraft.server.dialog` package (`CommonDialogData`, `NoticeDialog`,
//! `ConfirmationDialog`, `ActionButton`, `CommonButtonData`,
//! `body.PlainMessage`, `action.StaticAction`/`ActionTypes`) — never guessed.
//!
//! Deliberately out of phase-1 scope (see project memory for the full
//! rationale): the `server_links`/`dialog_list`/`multi_action` dialog types,
//! the `item` body type, `Input` controls (parsed away, not rendered), the
//! `show_dialog`/`change_page`/`suggest_command`/`custom` button actions and
//! `dynamic/run_command`'s `{}` template substitution (moot with no Inputs).

use azalea::Client;
use azalea::registry::identifier::Identifier;
use azalea::registry::{Holder, Registry, data::Dialog};
use azalea_chat::FormattedText;
use simdnbt::FromNbtTag;
use simdnbt::owned::{Nbt, NbtCompound, NbtTag};

use super::text::plain_text;

#[derive(Clone, Debug, PartialEq)]
pub struct DialogData {
    pub title: String,
    /// Shown in the pause-menu / pre-join dialog list in real vanilla; this
    /// client has no such list yet, kept for a future dialog_list pass.
    pub external_title: String,
    pub can_close_with_escape: bool,
    /// Real vanilla's `pause` distinguishes freezing a *singleplayer*
    /// integrated-server tick — this client has no such tick-freeze for any
    /// screen (the existing pause menu doesn't implement one either, see
    /// `Hud::is_paused`'s doc), so it's parsed for a future pass but doesn't
    /// change how a dialog renders or grabs input; every open dialog covers
    /// the HUD and releases the mouse the same way `sign`/`book`/`death` do.
    pub pause: bool,
    pub after_action: AfterAction,
    pub body: Vec<BodyEntry>,
    pub kind: DialogKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AfterAction {
    #[default]
    Close,
    None,
    WaitForResponse,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BodyEntry {
    PlainMessage { contents: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DialogKind {
    Notice { action: ActionButtonData },
    Confirmation { yes: ActionButtonData, no: ActionButtonData },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActionButtonData {
    pub label: String,
    pub tooltip: Option<String>,
    pub action: Option<ButtonAction>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ButtonAction {
    RunCommand(String),
    OpenUrl(String),
    CopyToClipboard(String),
}

impl DialogData {
    /// Real vanilla's `Dialog::onCancel` — what Escape runs (subject to
    /// `can_close_with_escape`): the Notice's only button, or Confirmation's
    /// "no" button. Both are real per-type overrides, not a generic default.
    pub fn on_cancel(&self) -> Option<&ButtonAction> {
        match &self.kind {
            DialogKind::Notice { action } => action.action.as_ref(),
            DialogKind::Confirmation { no, .. } => no.action.as_ref(),
        }
    }
}

/// Top-level entry point: resolve a `Holder<Dialog, Nbt>` — either the raw
/// NBT the server sent inline (`Direct`), or a protocol id into the synced
/// `minecraft:dialog` data registry (`Reference`, same "Nth key is protocol
/// id N" convention `read_variant_registry` already relies on) — into parsed
/// dialog data. `None` for any dialog type/body/shape outside phase-1 scope,
/// or malformed data (never partially renders a dialog it didn't understand).
pub fn parse_holder(bot: &Client, holder: &Holder<Dialog, Nbt>) -> Option<DialogData> {
    match holder {
        Holder::Direct(nbt) => {
            let Nbt::Some(base) = nbt else { return None };
            parse_compound(base)
        }
        Holder::Reference(id) => {
            let world = bot.world();
            let world = world.read();
            let key = Identifier::new("minecraft:dialog");
            let reg = world.registries.extra.get(&key)?;
            let (_, compound) = reg.map.get_index(id.to_u32() as usize)?;
            parse_compound(compound)
        }
    }
}

fn parse_compound(compound: &NbtCompound) -> Option<DialogData> {
    let type_id = compound.string("type")?.to_string();
    let type_id = type_id.strip_prefix("minecraft:").unwrap_or(&type_id);

    let title = component_text(compound.get("title")?)?;
    let external_title =
        compound.get("external_title").and_then(component_text).unwrap_or_else(|| title.clone());
    let can_close_with_escape = compound.byte("can_close_with_escape").map(|b| b != 0).unwrap_or(true);
    let pause = compound.byte("pause").map(|b| b != 0).unwrap_or(true);
    let after_action = compound
        .string("after_action")
        .map(|s| match s.to_string().as_str() {
            "none" => AfterAction::None,
            "wait_for_response" => AfterAction::WaitForResponse,
            _ => AfterAction::Close,
        })
        .unwrap_or_default();
    let body = parse_body(compound);

    let kind = match type_id {
        "notice" => {
            let action = compound
                .get("action")
                .and_then(|t| t.compound())
                .and_then(parse_action_button)
                .unwrap_or_else(default_notice_action);
            DialogKind::Notice { action }
        }
        "confirmation" => {
            let yes = compound.get("yes")?.compound().and_then(parse_action_button)?;
            let no = compound.get("no")?.compound().and_then(parse_action_button)?;
            DialogKind::Confirmation { yes, no }
        }
        // server_links/dialog_list/multi_action: real, but their own whole
        // screen types (dialog_list in particular needs paging) — deferred.
        _ => return None,
    };

    Some(DialogData { title, external_title, can_close_with_escape, pause, after_action, body, kind })
}

/// `CommonDialogData.body`: `DialogBody.COMPACT_LIST_CODEC` accepts either a
/// single body object directly, or a JSON/NBT list of them — never guess
/// which; missing/malformed entries are dropped, not fabricated.
fn parse_body(compound: &NbtCompound) -> Vec<BodyEntry> {
    let Some(tag) = compound.get("body") else { return Vec::new() };
    let compounds: Vec<&NbtCompound> = if let Some(list) = tag.list() {
        list.compounds().map(|s| s.iter().collect()).unwrap_or_default()
    } else if let Some(c) = tag.compound() {
        vec![c]
    } else {
        Vec::new()
    };
    compounds.iter().filter_map(|c| parse_body_entry(c)).collect()
}

fn parse_body_entry(compound: &NbtCompound) -> Option<BodyEntry> {
    let type_id = compound.string("type")?.to_string();
    let type_id = type_id.strip_prefix("minecraft:").unwrap_or(&type_id);
    match type_id {
        // `item` bodies need a per-item render (icon + optional description)
        // this pass doesn't build — deferred, not fabricated as text.
        "plain_message" => {
            let contents = component_text(compound.get("contents")?)?;
            Some(BodyEntry::PlainMessage { contents })
        }
        _ => None,
    }
}

fn parse_action_button(compound: &NbtCompound) -> Option<ActionButtonData> {
    let label = component_text(compound.get("label")?)?;
    let tooltip = compound.get("tooltip").and_then(component_text);
    let action = compound.get("action").and_then(|t| t.compound()).and_then(parse_button_action);
    Some(ActionButtonData { label, tooltip, action })
}

/// The button's `Action` registry dispatch (`minecraft:dialog_action_type`,
/// key `"type"`). Real vanilla's non-`dynamic/*` ids are each just the
/// wrapped `ClickEvent.Action`'s own field shape (`StaticAction`,
/// decompiled) — phase-1 covers the three that need no further engine
/// plumbing (chat's existing `run_command`/`open_url`/`copy_to_clipboard`
/// handling, see `app/chat.rs`'s `ChatClick`). `show_dialog` (nested
/// dialogs), `change_page`, `suggest_command` and `custom` are real but
/// deferred; a button with none of these (including a totally absent
/// `action` key) is `None` — it just runs `after_action`, per `ActionButton`
/// itself only `optionalFieldOf`-ing the whole key.
fn parse_button_action(compound: &NbtCompound) -> Option<ButtonAction> {
    let type_id = compound.string("type")?.to_string();
    let type_id = type_id.strip_prefix("minecraft:").unwrap_or(&type_id);
    match type_id {
        "run_command" => Some(ButtonAction::RunCommand(compound.string("command")?.to_string())),
        "open_url" => Some(ButtonAction::OpenUrl(compound.string("url")?.to_string())),
        "copy_to_clipboard" => Some(ButtonAction::CopyToClipboard(compound.string("value")?.to_string())),
        _ => None,
    }
}

fn default_notice_action() -> ActionButtonData {
    // `NoticeDialog.DEFAULT_ACTION`: real `gui.ok` en_us string is "Ok"
    // (capital O, lowercase k — verified against the extracted lang file,
    // not "OK"), no click action (just runs `after_action`).
    ActionButtonData { label: "Ok".to_string(), tooltip: None, action: None }
}

/// A raw owned NBT tag → `FormattedText` → flattened plain string, the same
/// simplification this codebase already applies to other locally-rendered
/// UI labels sourced from a real server Component (see `server_link()` in
/// `bridge/mod.rs`, which does the same for `ClientboundServerLinks`).
/// `FormattedText::from_nbt_tag` only accepts a *borrowed* simdnbt tag (it's
/// built for the network decoder), so this round-trips through the same
/// `[type_id][payload]` byte encoding `FormattedText`'s own `AzBuf` impl
/// reads — not a hack, the identical trick that impl already uses via
/// `simdnbt::borrow::read_optional_tag`.
fn component_text(tag: &NbtTag) -> Option<String> {
    let mut bytes = Vec::new();
    tag.write(&mut bytes);
    let mut cursor = std::io::Cursor::new(bytes.as_slice());
    let borrowed = simdnbt::borrow::read_optional_tag(&mut cursor).ok().flatten()?;
    let ft = FormattedText::from_nbt_tag(borrowed.as_tag())?;
    Some(plain_text(&ft))
}

#[cfg(test)]
mod tests {
    use super::*;
    use simdnbt::owned::NbtCompound;

    fn text_tag(s: &str) -> NbtTag {
        NbtTag::String(s.into())
    }

    fn title_only(mut c: NbtCompound, title: &str) -> NbtCompound {
        c.insert("title", text_tag(title));
        c
    }

    #[test]
    fn notice_uses_real_default_ok_button_when_action_absent() {
        let mut c = NbtCompound::new();
        c.insert("type", text_tag("notice"));
        let c = title_only(c, "Hello");
        let d = parse_compound(&c).unwrap();
        assert_eq!(d.title, "Hello");
        assert_eq!(d.after_action, AfterAction::Close);
        assert!(d.can_close_with_escape);
        assert!(d.pause);
        match d.kind {
            DialogKind::Notice { action } => {
                assert_eq!(action.label, "Ok");
                assert_eq!(action.action, None);
            }
            _ => panic!("expected Notice"),
        }
    }

    #[test]
    fn confirmation_requires_both_buttons() {
        let mut c = NbtCompound::new();
        c.insert("type", text_tag("confirmation"));
        let c = title_only(c, "Sure?");
        // Missing "yes"/"no" — real codec requires both, no defaults.
        assert!(parse_compound(&c).is_none());
    }

    #[test]
    fn confirmation_parses_yes_no_actions() {
        let mut yes_btn = NbtCompound::new();
        yes_btn.insert("label", text_tag("Yes"));
        let mut yes_action = NbtCompound::new();
        yes_action.insert("type", text_tag("run_command"));
        yes_action.insert("command", text_tag("/spawn"));
        yes_btn.insert("action", NbtTag::Compound(yes_action));

        let mut no_btn = NbtCompound::new();
        no_btn.insert("label", text_tag("No"));

        let mut c = NbtCompound::new();
        c.insert("type", text_tag("confirmation"));
        c.insert("title", text_tag("Sure?"));
        c.insert("yes", NbtTag::Compound(yes_btn));
        c.insert("no", NbtTag::Compound(no_btn));

        let d = parse_compound(&c).unwrap();
        match &d.kind {
            DialogKind::Confirmation { yes, no } => {
                assert_eq!(yes.action, Some(ButtonAction::RunCommand("/spawn".to_string())));
                assert_eq!(no.action, None);
            }
            _ => panic!("expected Confirmation"),
        }
        assert_eq!(d.on_cancel(), None); // Escape → "no" button's action
    }

    #[test]
    fn optional_fields_fall_back_to_real_defaults() {
        let mut c = NbtCompound::new();
        c.insert("type", text_tag("notice"));
        c.insert("title", text_tag("T"));
        c.insert("can_close_with_escape", NbtTag::Byte(0));
        c.insert("pause", NbtTag::Byte(0));
        c.insert("after_action", text_tag("wait_for_response"));
        let d = parse_compound(&c).unwrap();
        assert!(!d.can_close_with_escape);
        assert!(!d.pause);
        assert_eq!(d.after_action, AfterAction::WaitForResponse);
    }

    #[test]
    fn compact_body_accepts_single_object_or_list() {
        let mut msg = NbtCompound::new();
        msg.insert("type", text_tag("plain_message"));
        msg.insert("contents", text_tag("Hi"));
        let mut c = NbtCompound::new();
        c.insert("type", text_tag("notice"));
        c.insert("title", text_tag("T"));
        c.insert("body", NbtTag::Compound(msg));
        let d = parse_compound(&c).unwrap();
        assert_eq!(d.body, vec![BodyEntry::PlainMessage { contents: "Hi".to_string() }]);
    }

    #[test]
    fn unknown_dialog_type_is_none_not_fabricated() {
        let mut c = NbtCompound::new();
        c.insert("type", text_tag("server_links"));
        c.insert("title", text_tag("T"));
        assert!(parse_compound(&c).is_none());
    }
}
