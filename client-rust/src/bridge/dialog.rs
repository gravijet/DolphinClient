//! Parses vanilla's `ClientboundShowDialog` payload — a raw, server-authored
//! NBT document azalea does not know the schema of at all (`Holder<Dialog,
//! Nbt>`) — into the subset of the real "Dialogs" system this client
//! renders: `notice`/`confirmation` dialogs with `plain_message`/`item`
//! bodies and a bounded [`ButtonAction`] set. Every field name, default and
//! dispatch key below is transcribed from the decompiled 26.1 client jar's
//! `net.minecraft.server.dialog` package (`CommonDialogData`, `NoticeDialog`,
//! `ConfirmationDialog`, `ActionButton`, `CommonButtonData`,
//! `body.PlainMessage`, `body.ItemBody`, `action.StaticAction`/`ActionTypes`/
//! `CommandTemplate`, `net.minecraft.commands.functions.StringTemplate`) —
//! never guessed.
//!
//! Deliberately still out of scope (see project memory for the full
//! rationale): the `server_links`/`dialog_list`/`multi_action` dialog types,
//! `Input` controls (parsed away, not rendered — no serverbound "dialog
//! response" packet exists yet to report a value back anyway) and the
//! `change_page`/`custom` button actions (both real, but `change_page` only
//! makes sense once `dialog_list` paging exists, and `custom` is an opaque
//! server-defined payload with nothing generic to do with it).

use azalea::Client;
use azalea::registry::identifier::Identifier;
use azalea::registry::{Holder, Registry, data::Dialog};
use azalea_chat::FormattedText;
use indexmap::IndexMap;
use simdnbt::FromNbtTag;
use simdnbt::owned::{Nbt, NbtCompound, NbtTag};

use super::events::ItemSnapshot;
use super::text::plain_text;

/// The synced `minecraft:dialog` data registry, snapshotted once per
/// `parse_holder` call and threaded down to every nested parse function that
/// might need it (currently only a `show_dialog` button's by-name reference,
/// [`parse_nested_dialog_holder`]) — avoids re-locking `bot.world()` per
/// nesting level, and keeps every parse function below testable without a
/// live `azalea::Client` (tests just pass `None`).
type DialogRegistry<'a> = Option<&'a IndexMap<Identifier, NbtCompound>>;

/// How deep a chain of `show_dialog` buttons may nest before this client
/// gives up and drops the innermost link (`None`) rather than recursing
/// forever. Real vanilla has no such limit (an arbitrarily deep dialog chain
/// is legal), but a small, generous bound is cheap insurance against a
/// pathological or hostile server without changing any legitimate dialog's
/// behavior — no real dialog flow nests anywhere close to this deep.
const MAX_DIALOG_NESTING: u8 = 8;

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
    Item {
        item: ItemSnapshot,
        description: Option<String>,
        show_decorations: bool,
        show_tooltip: bool,
        /// Real `ItemBody.width`/`.height` (both `intRange(1,256)`, default
        /// 16) — the icon's own rendered size in GUI px, NOT a text-wrap
        /// width (unlike `PlainMessage.width`); `description` wraps at the
        /// same fixed default `PlainMessage` itself uses, since `ItemBody`
        /// carries no wrap-width field of its own.
        icon_width: i32,
        icon_height: i32,
    },
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
    SuggestCommand(String),
    /// `Box` because `DialogData` recursively contains `ButtonAction`.
    ShowDialog(Box<DialogData>),
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
    let world = bot.world();
    let world = world.read();
    let key = Identifier::new("minecraft:dialog");
    let registry: DialogRegistry = world.registries.extra.get(&key).map(|r| &r.map);

    match holder {
        Holder::Direct(nbt) => {
            let Nbt::Some(base) = nbt else { return None };
            parse_compound(registry, base, 0)
        }
        Holder::Reference(id) => {
            let (_, compound) = registry?.get_index(id.to_u32() as usize)?;
            parse_compound(registry, compound, 0)
        }
    }
}

/// Real `Dialog.CODEC = RegistryFileCodec.create(Registries.DIALOG,
/// DIRECT_CODEC)` — used (unlike the packet-level `Holder<Dialog, Nbt>`
/// `parse_holder` resolves, which is a *wire*-level Direct/Reference split)
/// wherever a `Holder<Dialog>` is nested inside ordinary NBT data, i.e. a
/// `show_dialog` button action's own `dialog` field. `RegistryFileCodec`'s
/// own convention: a bare string is a *by-name* reference into the
/// `minecraft:dialog` registry (not a protocol id — looked up by key, unlike
/// `parse_holder`'s `Reference` case which is looked up by index), a compound
/// is the dialog's own inline data.
fn parse_nested_dialog_holder(registry: DialogRegistry, tag: &NbtTag, depth: u8) -> Option<DialogData> {
    if depth >= MAX_DIALOG_NESTING {
        return None;
    }
    if let Some(name) = tag.string() {
        let name = name.to_string();
        let name = if name.contains(':') { name } else { format!("minecraft:{name}") };
        let compound = registry?.get(&Identifier::new(&name))?;
        parse_compound(registry, compound, depth + 1)
    } else {
        parse_compound(registry, tag.compound()?, depth + 1)
    }
}

fn parse_compound(registry: DialogRegistry, compound: &NbtCompound, depth: u8) -> Option<DialogData> {
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
                .and_then(|c| parse_action_button(registry, c, depth))
                .unwrap_or_else(default_notice_action);
            DialogKind::Notice { action }
        }
        "confirmation" => {
            let yes =
                compound.get("yes")?.compound().and_then(|c| parse_action_button(registry, c, depth))?;
            let no = compound.get("no")?.compound().and_then(|c| parse_action_button(registry, c, depth))?;
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
        "plain_message" => {
            let contents = component_text(compound.get("contents")?)?;
            Some(BodyEntry::PlainMessage { contents })
        }
        "item" => parse_item_body(compound),
        _ => None,
    }
}

/// `ItemBody(item: ItemStackTemplate, description: Optional<PlainMessage>,
/// show_decorations: bool = true, show_tooltip: bool = true, width/height:
/// int = 16)`. `ItemStackTemplate`'s own `id`/`count`/`components` mirror
/// `ItemStack`'s NBT shape (`Codec.withAlternative` also allows a bare item
/// id with no wrapper, same as any other `ItemStackTemplate` field); this
/// only reads `id`/`count` into the same [`ItemSnapshot`] every other item
/// render already uses, via the existing generic id-keyed icon/tooltip
/// lookup (`container::draw_item`/`container::tooltip`) — deliberately not
/// parsing `components` (a full `DataComponentPatch`, the same scope as the
/// rest of this client's tooltip system) means a custom name/lore/enchant
/// glint from THIS SPECIFIC dialog item won't show, only its default
/// (still-correct) name/icon/tooltip — a bounded, documented simplification,
/// not fabricated data.
fn parse_item_body(compound: &NbtCompound) -> Option<BodyEntry> {
    let item_tag = compound.get("item")?;
    let item_compound = item_tag.compound();
    let (id_str, count) = if let Some(c) = item_compound {
        let id = c.string("id")?.to_string();
        let count = c.int("count").unwrap_or(1).max(1) as u32;
        (id, count)
    } else {
        (item_tag.string()?.to_string(), 1)
    };
    let id = id_str.strip_prefix("minecraft:").unwrap_or(&id_str).to_string();

    let description = compound.get("description").and_then(parse_plain_message_field);
    let show_decorations = compound.byte("show_decorations").map(|b| b != 0).unwrap_or(true);
    let show_tooltip = compound.byte("show_tooltip").map(|b| b != 0).unwrap_or(true);
    let icon_width = compound.int("width").unwrap_or(16).clamp(1, 256);
    let icon_height = compound.int("height").unwrap_or(16).clamp(1, 256);

    Some(BodyEntry::Item {
        item: ItemSnapshot { item: id, count, ..ItemSnapshot::default() },
        description,
        show_decorations,
        show_tooltip,
        icon_width,
        icon_height,
    })
}

/// `PlainMessage.CODEC = Codec.withAlternative(MAP_CODEC, ComponentSerialization.CODEC, ...)`
/// — used directly (not through `DialogBody`'s "type"-tagged registry
/// dispatch) for `ItemBody.description`: either `{"contents": ..., "width":
/// ...}` or a bare Component, with no "type" key either way.
fn parse_plain_message_field(tag: &NbtTag) -> Option<String> {
    if let Some(c) = tag.compound() {
        if let Some(contents) = c.get("contents") {
            return component_text(contents);
        }
    }
    component_text(tag)
}

fn parse_action_button(
    registry: DialogRegistry,
    compound: &NbtCompound,
    depth: u8,
) -> Option<ActionButtonData> {
    let label = component_text(compound.get("label")?)?;
    let tooltip = compound.get("tooltip").and_then(component_text);
    let action = compound
        .get("action")
        .and_then(|t| t.compound())
        .and_then(|c| parse_button_action(registry, c, depth));
    Some(ActionButtonData { label, tooltip, action })
}

/// The button's `Action` registry dispatch (`minecraft:dialog_action_type`,
/// key `"type"`). Real vanilla's non-`dynamic/*` ids are each just the
/// wrapped `ClickEvent.Action`'s own field shape (`StaticAction`, decompiled)
/// — covers every `isAllowedFromServer()` variant except `change_page`
/// (needs `dialog_list` paging, not built) and `custom` (opaque payload).
/// `dynamic/run_command` (`CommandTemplate`, decompiled) substitutes via a
/// real `$(var)`-marker [`ParsedTemplate`]/`StringTemplate`
/// (`net.minecraft.commands.functions.StringTemplate.fromString`,
/// decompiled — NOT `{}`, that was an earlier, wrong guess) with an empty
/// argument map, since this client has no `Input` controls to source real
/// values from yet (`StringTemplate.instantiate`'s own real fallback for a
/// missing argument is `""`, so this is a correct instance of real vanilla
/// behavior, not an invented shortcut). A button with none of these
/// (including a totally absent `action` key) is `None` — it just runs
/// `after_action`, per `ActionButton` itself only `optionalFieldOf`-ing the
/// whole key.
fn parse_button_action(registry: DialogRegistry, compound: &NbtCompound, depth: u8) -> Option<ButtonAction> {
    let type_id = compound.string("type")?.to_string();
    let type_id = type_id.strip_prefix("minecraft:").unwrap_or(&type_id);
    match type_id {
        "run_command" => Some(ButtonAction::RunCommand(compound.string("command")?.to_string())),
        "open_url" => Some(ButtonAction::OpenUrl(compound.string("url")?.to_string())),
        "copy_to_clipboard" => Some(ButtonAction::CopyToClipboard(compound.string("value")?.to_string())),
        "suggest_command" => {
            Some(ButtonAction::SuggestCommand(compound.string("command")?.to_string()))
        }
        "show_dialog" => {
            let nested = parse_nested_dialog_holder(registry, compound.get("dialog")?, depth)?;
            Some(ButtonAction::ShowDialog(Box::new(nested)))
        }
        "dynamic/run_command" => {
            let raw = compound.string("template")?.to_string();
            let template = template::ParsedTemplate::parse(&raw)?;
            Some(ButtonAction::RunCommand(template.instantiate(&std::collections::HashMap::new())))
        }
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

/// Byte-exact port of `net.minecraft.commands.functions.StringTemplate`
/// (decompiled) — `dynamic/run_command`'s `CommandTemplate.template` field
/// (`ParsedTemplate`, itself just this wrapped with a `raw` string kept for
/// re-serialization, irrelevant here) parses a command string containing
/// `$(varname)` markers, substituting each at click time. NOT `{}`-style —
/// that was an earlier, unverified guess corrected by this decompile.
mod template {
    #[derive(Clone, Debug, PartialEq)]
    pub struct ParsedTemplate {
        /// Literal text runs, interleaved with `variables` (`segments.len()`
        /// is `variables.len()` or `variables.len() + 1`, matching real
        /// vanilla's `ImmutableList.Builder` usage exactly).
        segments: Vec<String>,
        variables: Vec<String>,
    }

    impl ParsedTemplate {
        /// `StringTemplate.fromString`: scan for `$`, and only treat it as a
        /// marker start when immediately followed by `(`; the matching `)`
        /// ends the variable name. Real vanilla requires at least one
        /// variable (`"No variables in macro"` if none found) and validates
        /// each name is letters/digits/`_` only (`isValidVariableName`) —
        /// both real error cases, not this client's own invention.
        pub fn parse(input: &str) -> Option<Self> {
            let bytes = input.as_bytes();
            let len = bytes.len();
            let mut segments = Vec::new();
            let mut variables = Vec::new();
            let mut start = 0usize;
            let mut index = input.find('$');
            while let Some(i) = index {
                if i == len - 1 || bytes[i + 1] != b'(' {
                    index = input[i + 1..].find('$').map(|j| i + 1 + j);
                    continue;
                }
                segments.push(input[start..i].to_string());
                let var_end = input[i + 1..].find(')').map(|j| i + 1 + j)?;
                let variable = &input[i + 2..var_end];
                if !is_valid_variable_name(variable) {
                    return None;
                }
                variables.push(variable.to_string());
                start = var_end + 1;
                index = input[start..].find('$').map(|j| start + j);
            }
            if start == 0 {
                // Real vanilla: "No variables in macro" — a template with no
                // `$(...)` marker at all is invalid, not a plain literal.
                return None;
            }
            if start != len {
                segments.push(input[start..].to_string());
            }
            Some(ParsedTemplate { segments, variables })
        }

        /// `StringTemplate.substitute`: interleave each variable's resolved
        /// value (real vanilla: `arguments.getOrDefault(k, "")` — a missing
        /// key becomes an empty string, not an error) between the literal
        /// segments.
        pub fn instantiate(&self, arguments: &std::collections::HashMap<String, String>) -> String {
            let mut out = String::new();
            for (i, var) in self.variables.iter().enumerate() {
                out.push_str(&self.segments[i]);
                out.push_str(arguments.get(var).map(String::as_str).unwrap_or(""));
            }
            if self.segments.len() > self.variables.len() {
                out.push_str(self.segments.last().unwrap());
            }
            out
        }
    }

    fn is_valid_variable_name(s: &str) -> bool {
        !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn substitutes_single_variable() {
            let t = ParsedTemplate::parse("say $(msg)!").unwrap();
            let mut args = std::collections::HashMap::new();
            args.insert("msg".to_string(), "hi".to_string());
            assert_eq!(t.instantiate(&args), "say hi!");
        }

        #[test]
        fn missing_argument_defaults_to_empty_string() {
            let t = ParsedTemplate::parse("say $(msg)!").unwrap();
            assert_eq!(t.instantiate(&std::collections::HashMap::new()), "say !");
        }

        #[test]
        fn multiple_variables_interleave_correctly() {
            let t = ParsedTemplate::parse("tp $(x) $(y) $(z)").unwrap();
            let mut args = std::collections::HashMap::new();
            args.insert("x".to_string(), "1".to_string());
            args.insert("y".to_string(), "2".to_string());
            args.insert("z".to_string(), "3".to_string());
            assert_eq!(t.instantiate(&args), "tp 1 2 3");
        }

        #[test]
        fn no_variables_is_invalid() {
            assert!(ParsedTemplate::parse("say hi").is_none());
        }

        #[test]
        fn unterminated_variable_is_invalid() {
            assert!(ParsedTemplate::parse("say $(msg").is_none());
        }

        #[test]
        fn invalid_variable_name_is_rejected() {
            assert!(ParsedTemplate::parse("say $(my-var)").is_none());
        }

        #[test]
        fn dollar_not_followed_by_paren_is_literal() {
            let t = ParsedTemplate::parse("cost: $5 $(amount)").unwrap();
            let mut args = std::collections::HashMap::new();
            args.insert("amount".to_string(), "10".to_string());
            assert_eq!(t.instantiate(&args), "cost: $5 10");
        }
    }
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
        let d = parse_compound(None, &c, 0).unwrap();
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
        assert!(parse_compound(None, &c, 0).is_none());
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

        let d = parse_compound(None, &c, 0).unwrap();
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
        let d = parse_compound(None, &c, 0).unwrap();
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
        let d = parse_compound(None, &c, 0).unwrap();
        assert_eq!(d.body, vec![BodyEntry::PlainMessage { contents: "Hi".to_string() }]);
    }

    #[test]
    fn unknown_dialog_type_is_none_not_fabricated() {
        let mut c = NbtCompound::new();
        c.insert("type", text_tag("server_links"));
        c.insert("title", text_tag("T"));
        assert!(parse_compound(None, &c, 0).is_none());
    }

    #[test]
    fn suggest_command_action_parses() {
        let mut action = NbtCompound::new();
        action.insert("type", text_tag("suggest_command"));
        action.insert("command", text_tag("/msg Steve "));
        let mut btn = NbtCompound::new();
        btn.insert("label", text_tag("Reply"));
        btn.insert("action", NbtTag::Compound(action));
        let d = parse_action_button(None, &btn, 0).unwrap();
        assert_eq!(d.action, Some(ButtonAction::SuggestCommand("/msg Steve ".to_string())));
    }

    #[test]
    fn dynamic_run_command_substitutes_with_empty_args_when_no_inputs() {
        // No `Input` controls exist client-side yet, so every variable
        // resolves to "" — the real `StringTemplate.instantiate` fallback
        // for a missing argument, not a fabricated shortcut.
        let mut action = NbtCompound::new();
        action.insert("type", text_tag("dynamic/run_command"));
        action.insert("template", text_tag("tp $(player) 0 0 0"));
        let mut btn = NbtCompound::new();
        btn.insert("label", text_tag("Go"));
        btn.insert("action", NbtTag::Compound(action));
        let d = parse_action_button(None, &btn, 0).unwrap();
        assert_eq!(d.action, Some(ButtonAction::RunCommand("tp  0 0 0".to_string())));
    }

    #[test]
    fn dynamic_run_command_with_no_variables_is_invalid() {
        let mut action = NbtCompound::new();
        action.insert("type", text_tag("dynamic/run_command"));
        action.insert("template", text_tag("spawn"));
        let mut btn = NbtCompound::new();
        btn.insert("label", text_tag("Go"));
        btn.insert("action", NbtTag::Compound(action));
        let d = parse_action_button(None, &btn, 0).unwrap();
        assert_eq!(d.action, None);
    }

    #[test]
    fn show_dialog_action_resolves_inline_direct_dialog() {
        let mut inner = NbtCompound::new();
        inner.insert("type", text_tag("notice"));
        inner.insert("title", text_tag("Inner"));

        let mut action = NbtCompound::new();
        action.insert("type", text_tag("show_dialog"));
        action.insert("dialog", NbtTag::Compound(inner));

        let mut btn = NbtCompound::new();
        btn.insert("label", text_tag("More info"));
        btn.insert("action", NbtTag::Compound(action));

        let d = parse_action_button(None, &btn, 0).unwrap();
        match d.action {
            Some(ButtonAction::ShowDialog(nested)) => assert_eq!(nested.title, "Inner"),
            other => panic!("expected ShowDialog, got {other:?}"),
        }
    }

    #[test]
    fn show_dialog_nesting_is_bounded() {
        // Depth already at the cap: even a well-formed inline dialog must
        // not resolve, so a pathological/hostile chain can't blow the stack.
        let mut inner = NbtCompound::new();
        inner.insert("type", text_tag("notice"));
        inner.insert("title", text_tag("Inner"));
        let tag = NbtTag::Compound(inner);
        assert!(parse_nested_dialog_holder(None, &tag, MAX_DIALOG_NESTING).is_none());
    }

    #[test]
    fn item_body_parses_compact_and_object_item_forms() {
        let mut obj_item = NbtCompound::new();
        obj_item.insert("id", text_tag("minecraft:diamond"));
        obj_item.insert("count", NbtTag::Int(3));
        let mut c1 = NbtCompound::new();
        c1.insert("type", text_tag("item"));
        c1.insert("item", NbtTag::Compound(obj_item));
        let e1 = parse_body_entry(&c1).unwrap();
        match e1 {
            BodyEntry::Item { item, show_decorations, show_tooltip, icon_width, icon_height, .. } => {
                assert_eq!(item.item, "diamond");
                assert_eq!(item.count, 3);
                assert!(show_decorations);
                assert!(show_tooltip);
                assert_eq!(icon_width, 16);
                assert_eq!(icon_height, 16);
            }
            other => panic!("expected Item, got {other:?}"),
        }

        // The bare-id alternative (`Codec.withAlternative`): no wrapper
        // object at all, count defaults to 1.
        let mut c2 = NbtCompound::new();
        c2.insert("type", text_tag("item"));
        c2.insert("item", text_tag("minecraft:stick"));
        let e2 = parse_body_entry(&c2).unwrap();
        match e2 {
            BodyEntry::Item { item, .. } => {
                assert_eq!(item.item, "stick");
                assert_eq!(item.count, 1);
            }
            other => panic!("expected Item, got {other:?}"),
        }
    }

    #[test]
    fn item_body_description_accepts_bare_component_or_object() {
        let mut item = NbtCompound::new();
        item.insert("id", text_tag("minecraft:apple"));

        let mut c1 = NbtCompound::new();
        c1.insert("type", text_tag("item"));
        c1.insert("item", NbtTag::Compound(item.clone()));
        c1.insert("description", text_tag("A tasty snack"));
        let e1 = parse_body_entry(&c1).unwrap();
        assert_eq!(
            e1,
            BodyEntry::Item {
                item: ItemSnapshot { item: "apple".to_string(), count: 1, ..ItemSnapshot::default() },
                description: Some("A tasty snack".to_string()),
                show_decorations: true,
                show_tooltip: true,
                icon_width: 16,
                icon_height: 16,
            }
        );

        let mut desc_obj = NbtCompound::new();
        desc_obj.insert("contents", text_tag("Wrapped form"));
        let mut c2 = NbtCompound::new();
        c2.insert("type", text_tag("item"));
        c2.insert("item", NbtTag::Compound(item));
        c2.insert("description", NbtTag::Compound(desc_obj));
        let e2 = parse_body_entry(&c2).unwrap();
        match e2 {
            BodyEntry::Item { description, .. } => {
                assert_eq!(description, Some("Wrapped form".to_string()))
            }
            other => panic!("expected Item, got {other:?}"),
        }
    }
}
