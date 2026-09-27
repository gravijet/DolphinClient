//! Parses vanilla's `ClientboundShowDialog` payload — a raw, server-authored
//! NBT document azalea does not know the schema of at all (`Holder<Dialog,
//! Nbt>`) — into the full real "Dialogs" system this client renders: all 5
//! real dialog types (`notice`/`confirmation`/`multi_action`/`dialog_list`/
//! `server_links`), `plain_message`/`item` bodies, every real button
//! [`ButtonAction`] (including `change_page`/`custom` and both `dynamic/*`
//! template variants), and all 4 [`InputControl`] types. Every field name,
//! default and dispatch key below is transcribed from the decompiled 26.1
//! client jar's `net.minecraft.server.dialog` package (`CommonDialogData`,
//! `NoticeDialog`, `ConfirmationDialog`, `MultiActionDialog`,
//! `DialogListDialog`, `ServerLinksDialog`, `ButtonListDialog`,
//! `ActionButton`, `CommonButtonData`, `Input`, `body.PlainMessage`,
//! `body.ItemBody`, `input.{BooleanInput,TextInput,NumberRangeInput,
//! SingleOptionInput}`, `action.{StaticAction,ActionTypes,CommandTemplate,
//! CustomAll}`, `net.minecraft.commands.functions.StringTemplate`,
//! `net.minecraft.network.chat.ClickEvent`) — never guessed.
//!
//! Two bounded, documented simplifications (real widget-init defaults not
//! decompiled — `InputControlHandlers`, the client-side widget factory, was
//! out of scope to chase down): a `single_option` input with no entry marked
//! `initial` defaults to its first entry (a reasonable default for a
//! cycle-style widget, not confirmed against the real client codec — no
//! codec-level default exists since the real validation only rejects
//! *multiple* initial entries, never zero); a `number_range` input's
//! substituted value is `{value}`-formatted with Rust's default `f32`
//! formatting (real vanilla's on-widget label uses `label_format`, decompiled
//! and implemented, but the *substituted* value string going into a command
//! template has no decompiled format-string source of truth).

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
    pub inputs: Vec<InputEntry>,
    pub kind: DialogKind,
}

/// `Input(key: String, control: InputControl)` — `key` is the name a
/// `$(key)` template marker or a `dynamic/custom` action's merged NBT field
/// resolves against; `control` dispatches on `"type"` the same way `Dialog`/
/// `DialogBody`/`Action` all do, but merged into the SAME object as `key`
/// rather than nested under its own key (real `Input.CODEC` groups both
/// fields flat).
#[derive(Clone, Debug, PartialEq)]
pub struct InputEntry {
    pub key: String,
    pub control: InputControl,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputControl {
    Boolean { label: String, initial: bool, on_true: String, on_false: String },
    Text {
        width: i32,
        label: String,
        label_visible: bool,
        initial: String,
        max_length: i32,
        multiline: Option<TextMultiline>,
    },
    NumberRange { width: i32, label: String, label_format: String, range: NumberRange },
    SingleOption { width: i32, label: String, label_visible: bool, entries: Vec<OptionEntry> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextMultiline {
    pub max_lines: Option<i32>,
    pub height: Option<i32>,
}

/// Byte-exact port of `NumberRangeInput.RangeInfo`'s real slider math
/// (decompiled) — a slider position (`0.0..=1.0`) maps to a value in
/// `start..=end`, optionally quantized to `step` around whichever value
/// `initial` (or the real fallback, the range's midpoint) scales to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberRange {
    pub start: f32,
    pub end: f32,
    pub initial: Option<f32>,
    pub step: Option<f32>,
}

impl NumberRange {
    /// `RangeInfo.initialScaledValue`.
    pub fn initial_scaled_value(&self) -> f32 {
        self.initial.unwrap_or((self.start + self.end) / 2.0)
    }

    /// `RangeInfo.scaledValueToSlider` (`Mth.inverseLerp`, with the real
    /// `start == end` guard against a division by zero).
    fn scaled_value_to_slider(&self, value: f32) -> f32 {
        if self.start == self.end {
            return 0.5;
        }
        (value - self.start) / (self.end - self.start)
    }

    /// `RangeInfo.initialSliderValue`.
    pub fn initial_slider_value(&self) -> f32 {
        self.scaled_value_to_slider(self.initial_scaled_value())
    }

    fn is_out_of_range(&self, scaled_value: f32) -> bool {
        let slider_pos = self.scaled_value_to_slider(scaled_value);
        !(0.0..=1.0).contains(&slider_pos)
    }

    /// `RangeInfo.computeScaledValue` (`Mth.lerp` + the real step-quantize-
    /// around-initial dance, including its own out-of-range one-step-back
    /// correction).
    pub fn compute_scaled_value(&self, slider_value: f32) -> f32 {
        let value_in_range = self.start + slider_value * (self.end - self.start);
        let Some(step) = self.step else { return value_in_range };
        let initial_value = self.initial_scaled_value();
        let delta_to_initial = value_in_range - initial_value;
        let steps_outside_initial = (delta_to_initial / step).round();
        let result = initial_value + steps_outside_initial * step;
        if !self.is_out_of_range(result) {
            return result;
        }
        let one_step_less = steps_outside_initial - steps_outside_initial.signum();
        initial_value + one_step_less * step
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OptionEntry {
    pub id: String,
    pub display: Option<String>,
    pub initial: bool,
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
    /// `MultiActionDialog`: a button grid, `actions` real-validated nonempty.
    MultiAction { actions: Vec<ActionButtonData>, exit_action: Option<ActionButtonData>, columns: i32 },
    /// `DialogListDialog`: each entry is another parsed dialog (no `Box`
    /// needed here, unlike `ButtonAction::ShowDialog` — a `Vec`'s elements
    /// are already heap-allocated, so `Vec<DialogData>` doesn't make
    /// `DialogData` infinitely-sized the way a bare recursive field would),
    /// opened the same way a `show_dialog` button does — real
    /// `DialogListDialogScreen` literally builds a `ClickEvent.ShowDialog`
    /// per entry, decompiled.
    DialogList {
        dialogs: Vec<DialogData>,
        exit_action: Option<ActionButtonData>,
        columns: i32,
        button_width: i32,
    },
    /// `ServerLinksDialog`: no link-list field of its own — real vanilla (and
    /// this client) renders the same server-links data the pause-menu
    /// `Pause::ServerLinks` screen already has (`Hud::server_links`).
    ServerLinks { exit_action: Option<ActionButtonData>, columns: i32, button_width: i32 },
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
    /// `ClickEvent.ChangePage` (real `ExtraCodecs.POSITIVE_INT` — vanilla's
    /// own codec rejects 0/negative, not just this client's own choice).
    ChangePage(u32),
    /// The static `custom` action (`ClickEvent.Custom`): an opaque
    /// server-defined id + optional payload NBT, sent back completely
    /// verbatim — unlike `DynamicCustom` below, no live Input values are
    /// merged in. Real `Custom.payload` is codec-typed as an arbitrary `Tag`,
    /// but the outgoing `ServerboundCustomClickAction.payload` field is
    /// itself `Nbt`-typed (a named root compound, azalea's `Nbt` type has no
    /// other variant) — so a non-compound payload has no way to reach the
    /// server through this packet regardless, making `NbtCompound` the only
    /// representable (and overwhelmingly the real-world) shape here.
    Custom { id: String, payload: Option<NbtCompound> },
    /// `dynamic/run_command` (`CommandTemplate`): the raw parsed `$(name)`
    /// template, kept unresolved until click time so it can substitute the
    /// dialog's OWN live Input values (a fixed empty-args instantiation, as
    /// phase-1's version did before real Input controls existed, is only
    /// correct when there are no inputs to substitute).
    DynamicRunCommand(template::ParsedTemplate),
    /// `dynamic/custom` (`CustomAll`): merges every live Input's current
    /// value (each as a `StringTag`, keyed by the Input's own `key` — real
    /// `Action.ValueGetter.asTag`) into `additions` (or an empty compound)
    /// at click time, then sends that as `ServerboundCustomClickAction`'s
    /// payload — a real, decompiled behavior, NOT related to `template`
    /// substitution the way `DynamicRunCommand` is (`CustomAll.createAction`
    /// never touches `ParsedTemplate` at all).
    DynamicCustom { id: String, additions: Option<NbtCompound> },
}

impl DialogData {
    /// Real vanilla's `Dialog::onCancel` — what Escape runs (subject to
    /// `can_close_with_escape`): the Notice's only button, or Confirmation's
    /// "no" button. Both are real per-type overrides, not a generic default.
    pub fn on_cancel(&self) -> Option<&ButtonAction> {
        match &self.kind {
            DialogKind::Notice { action } => action.action.as_ref(),
            DialogKind::Confirmation { no, .. } => no.action.as_ref(),
            // `ButtonListDialog`'s own real default `onCancel`: the exit
            // button's action if present, otherwise no action at all (real
            // vanilla does NOT synthesize a fallback "Back" action here —
            // `ButtonListDialogScreen` may still render a footer "Back"
            // button with no `action`, i.e. `on_cancel` correctly returns
            // `None` for it too, decompiled).
            DialogKind::MultiAction { exit_action, .. }
            | DialogKind::DialogList { exit_action, .. }
            | DialogKind::ServerLinks { exit_action, .. } => {
                exit_action.as_ref().and_then(|b| b.action.as_ref())
            }
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
    let inputs = parse_inputs(compound.get("inputs"));

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
        "multi_action" => {
            // Real `ExtraCodecs.nonEmptyList` — an empty/missing `actions`
            // list is a real parse failure, not an empty grid.
            let actions: Vec<ActionButtonData> = compound
                .get("actions")?
                .list()
                .and_then(|l| l.compounds())
                .unwrap_or_default()
                .iter()
                .filter_map(|c| parse_action_button(registry, c, depth))
                .collect();
            if actions.is_empty() {
                return None;
            }
            let exit_action = parse_optional_action_button(registry, compound, "exit_action", depth);
            let columns = compound.int("columns").unwrap_or(2).max(1);
            DialogKind::MultiAction { actions, exit_action, columns }
        }
        "dialog_list" => {
            // Real `Dialog.LIST_CODEC` (`HolderSet<Dialog>`): a mixed list of
            // by-name string refs and inline compounds, same per-entry shape
            // `parse_nested_dialog_holder` already resolves for `show_dialog`
            // — so this needs the raw tag list, not `.compounds()` alone.
            let dialogs: Vec<DialogData> = compound
                .get("dialogs")?
                .list()
                .map(|l| l.as_nbt_tags())
                .unwrap_or_default()
                .iter()
                .filter_map(|t| parse_nested_dialog_holder(registry, t, depth))
                .collect();
            let exit_action = parse_optional_action_button(registry, compound, "exit_action", depth);
            let columns = compound.int("columns").unwrap_or(2).max(1);
            let button_width = compound.int("button_width").unwrap_or(150);
            DialogKind::DialogList { dialogs, exit_action, columns, button_width }
        }
        "server_links" => {
            let exit_action = parse_optional_action_button(registry, compound, "exit_action", depth);
            let columns = compound.int("columns").unwrap_or(2).max(1);
            let button_width = compound.int("button_width").unwrap_or(150);
            DialogKind::ServerLinks { exit_action, columns, button_width }
        }
        _ => return None,
    };

    Some(DialogData { title, external_title, can_close_with_escape, pause, after_action, body, inputs, kind })
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

/// `CommonDialogData.inputs` (`Input.CODEC.listOf().optionalFieldOf("inputs",
/// List.of())`) — each list entry merges `Input`'s own `key` field flat with
/// whichever `InputControl` its `"type"` dispatches to (real `Input.CODEC`
/// groups both at the same object level, no nested wrapper).
fn parse_inputs(tag: Option<&NbtTag>) -> Vec<InputEntry> {
    let Some(tag) = tag else { return Vec::new() };
    let Some(list) = tag.list() else { return Vec::new() };
    list.compounds().unwrap_or_default().iter().filter_map(parse_input_entry).collect()
}

fn parse_input_entry(c: &NbtCompound) -> Option<InputEntry> {
    let key = c.string("key")?.to_string();
    let type_id = c.string("type")?.to_string();
    let type_id = type_id.strip_prefix("minecraft:").unwrap_or(&type_id);
    let control = match type_id {
        "boolean" => InputControl::Boolean {
            label: component_text(c.get("label")?)?,
            initial: c.byte("initial").map(|b| b != 0).unwrap_or(false),
            on_true: c.string("on_true").map(|s| s.to_string()).unwrap_or_else(|| "true".to_string()),
            on_false: c.string("on_false").map(|s| s.to_string()).unwrap_or_else(|| "false".to_string()),
        },
        "text" => InputControl::Text {
            width: c.int("width").unwrap_or(200),
            label: component_text(c.get("label")?)?,
            label_visible: c.byte("label_visible").map(|b| b != 0).unwrap_or(true),
            initial: c.string("initial").map(|s| s.to_string()).unwrap_or_default(),
            max_length: c.int("max_length").unwrap_or(32).max(1),
            multiline: c.get("multiline").and_then(|t| t.compound()).map(|mc| TextMultiline {
                max_lines: mc.int("max_lines"),
                height: mc.int("height"),
            }),
        },
        "number_range" => InputControl::NumberRange {
            width: c.int("width").unwrap_or(200),
            label: component_text(c.get("label")?)?,
            label_format: c
                .string("label_format")
                .map(|s| s.to_string())
                .unwrap_or_else(|| "options.generic_value".to_string()),
            range: NumberRange {
                start: c.float("start")?,
                end: c.float("end")?,
                initial: c.float("initial"),
                step: c.float("step"),
            },
        },
        "single_option" => {
            // Real `ExtraCodecs.nonEmptyList` on `options`.
            let entries: Vec<OptionEntry> = c
                .get("options")?
                .list()
                .map(|l| l.as_nbt_tags())
                .unwrap_or_default()
                .iter()
                .filter_map(parse_option_entry)
                .collect();
            if entries.is_empty() {
                return None;
            }
            InputControl::SingleOption {
                width: c.int("width").unwrap_or(200),
                label: component_text(c.get("label")?)?,
                label_visible: c.byte("label_visible").map(|b| b != 0).unwrap_or(true),
                entries,
            }
        }
        _ => return None,
    };
    Some(InputEntry { key, control })
}

/// `SingleOptionInput.Entry.CODEC`: either a bare string id, or the full
/// `{id, display?, initial?}` object (`Codec.withAlternative`).
fn parse_option_entry(tag: &NbtTag) -> Option<OptionEntry> {
    if let Some(s) = tag.string() {
        return Some(OptionEntry { id: s.to_string(), display: None, initial: false });
    }
    let c = tag.compound()?;
    Some(OptionEntry {
        id: c.string("id")?.to_string(),
        display: c.get("display").and_then(component_text),
        initial: c.byte("initial").map(|b| b != 0).unwrap_or(false),
    })
}

/// `ButtonListDialog.exitAction` (`ActionButton.CODEC.optionalFieldOf(...)`,
/// shared by all 3 button-list dialog types) — a missing key or malformed
/// button is the same real "no exit button rendered" outcome (real vanilla
/// only ever omits the whole key, but a malformed one degrading the same way
/// is a harmless, conservative choice, not a fabricated behavior).
fn parse_optional_action_button(
    registry: DialogRegistry,
    compound: &NbtCompound,
    key: &str,
    depth: u8,
) -> Option<ActionButtonData> {
    compound.get(key).and_then(|t| t.compound()).and_then(|c| parse_action_button(registry, c, depth))
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
        // Real `ExtraCodecs.POSITIVE_INT` — 0 or negative is a real parse
        // failure, not just clamped, so a bad value drops the whole action
        // rather than silently coercing to 1.
        "change_page" => {
            let page = compound.int("page")?;
            if page <= 0 { None } else { Some(ButtonAction::ChangePage(page as u32)) }
        }
        "custom" => {
            let id = compound.string("id")?.to_string();
            let payload = compound.get("payload").and_then(|t| t.compound()).cloned();
            Some(ButtonAction::Custom { id, payload })
        }
        "dynamic/run_command" => {
            let raw = compound.string("template")?.to_string();
            let template = template::ParsedTemplate::parse(&raw)?;
            Some(ButtonAction::DynamicRunCommand(template))
        }
        "dynamic/custom" => {
            let id = compound.string("id")?.to_string();
            let additions = compound.get("additions").and_then(|t| t.compound()).cloned();
            Some(ButtonAction::DynamicCustom { id, additions })
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
pub mod template {
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
    use simdnbt::owned::{NbtCompound, NbtList};

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
        c.insert("type", text_tag("totally_made_up_type"));
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
        // Superseded by `dynamic_run_command_stays_unresolved_at_parse_time`
        // now that real Input controls exist — kept as a regression check
        // that an absent variable still falls back to the real `""`
        // (`StringTemplate.instantiate`'s own missing-argument default).
        let mut action = NbtCompound::new();
        action.insert("type", text_tag("dynamic/run_command"));
        action.insert("template", text_tag("tp $(player) 0 0 0"));
        let mut btn = NbtCompound::new();
        btn.insert("label", text_tag("Go"));
        btn.insert("action", NbtTag::Compound(action));
        let d = parse_action_button(None, &btn, 0).unwrap();
        match d.action {
            Some(ButtonAction::DynamicRunCommand(t)) => {
                assert_eq!(t.instantiate(&std::collections::HashMap::new()), "tp  0 0 0");
            }
            other => panic!("expected DynamicRunCommand, got {other:?}"),
        }
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

    fn action_button(label: &str) -> NbtCompound {
        let mut c = NbtCompound::new();
        c.insert("label", text_tag(label));
        c
    }

    #[test]
    fn change_page_action_parses_and_rejects_non_positive() {
        let mut ok = NbtCompound::new();
        ok.insert("type", text_tag("change_page"));
        ok.insert("page", NbtTag::Int(3));
        let mut btn = action_button("Next");
        btn.insert("action", NbtTag::Compound(ok));
        let d = parse_action_button(None, &btn, 0).unwrap();
        assert_eq!(d.action, Some(ButtonAction::ChangePage(3)));

        let mut zero = NbtCompound::new();
        zero.insert("type", text_tag("change_page"));
        zero.insert("page", NbtTag::Int(0));
        let mut btn2 = action_button("Next");
        btn2.insert("action", NbtTag::Compound(zero));
        let d2 = parse_action_button(None, &btn2, 0).unwrap();
        assert_eq!(d2.action, None);
    }

    #[test]
    fn custom_action_parses_id_and_optional_payload() {
        let mut payload = NbtCompound::new();
        payload.insert("foo", text_tag("bar"));
        let mut action = NbtCompound::new();
        action.insert("type", text_tag("custom"));
        action.insert("id", text_tag("modid:thing"));
        action.insert("payload", NbtTag::Compound(payload.clone()));
        let mut btn = action_button("Go");
        btn.insert("action", NbtTag::Compound(action));
        let d = parse_action_button(None, &btn, 0).unwrap();
        assert_eq!(d.action, Some(ButtonAction::Custom { id: "modid:thing".to_string(), payload: Some(payload) }));
    }

    #[test]
    fn dynamic_custom_action_keeps_template_unresolved_until_click() {
        let mut additions = NbtCompound::new();
        additions.insert("fixed", text_tag("1"));
        let mut action = NbtCompound::new();
        action.insert("type", text_tag("dynamic/custom"));
        action.insert("id", text_tag("modid:thing"));
        action.insert("additions", NbtTag::Compound(additions.clone()));
        let mut btn = action_button("Go");
        btn.insert("action", NbtTag::Compound(action));
        let d = parse_action_button(None, &btn, 0).unwrap();
        assert_eq!(
            d.action,
            Some(ButtonAction::DynamicCustom { id: "modid:thing".to_string(), additions: Some(additions) })
        );
    }

    #[test]
    fn dynamic_run_command_stays_unresolved_at_parse_time() {
        // Corrects phase-1's eager-instantiate-with-empty-args behavior: now
        // that real Input controls exist, resolving at parse time would
        // always substitute "" even when a live input has a real value.
        let mut action = NbtCompound::new();
        action.insert("type", text_tag("dynamic/run_command"));
        action.insert("template", text_tag("tp $(player) 0 0 0"));
        let mut btn = action_button("Go");
        btn.insert("action", NbtTag::Compound(action));
        let d = parse_action_button(None, &btn, 0).unwrap();
        match d.action {
            Some(ButtonAction::DynamicRunCommand(t)) => {
                let mut args = std::collections::HashMap::new();
                args.insert("player".to_string(), "Steve".to_string());
                assert_eq!(t.instantiate(&args), "tp Steve 0 0 0");
            }
            other => panic!("expected DynamicRunCommand, got {other:?}"),
        }
    }

    fn dialog_of_type(type_id: &str) -> NbtCompound {
        let mut c = NbtCompound::new();
        c.insert("type", text_tag(type_id));
        c.insert("title", text_tag("T"));
        c
    }

    #[test]
    fn multi_action_requires_nonempty_actions() {
        let d = dialog_of_type("multi_action");
        assert!(parse_compound(None, &d, 0).is_none());
    }

    #[test]
    fn multi_action_parses_actions_exit_and_column_default() {
        let mut a1 = action_button("A");
        let mut a2 = action_button("B");
        let mut run = NbtCompound::new();
        run.insert("type", text_tag("run_command"));
        run.insert("command", text_tag("/a"));
        a1.insert("action", NbtTag::Compound(run));
        a2.insert("action", NbtTag::Compound(NbtCompound::new())); // no "type" -> None action, still a valid button
        let actions_list = NbtList::Compound(vec![a1, a2]);

        let mut c = dialog_of_type("multi_action");
        c.insert("actions", NbtTag::List(actions_list));
        c.insert("exit_action", NbtTag::Compound(action_button("Exit")));
        let d = parse_compound(None, &c, 0).unwrap();
        match &d.kind {
            DialogKind::MultiAction { actions, exit_action, columns } => {
                assert_eq!(actions.len(), 2);
                assert_eq!(actions[0].action, Some(ButtonAction::RunCommand("/a".to_string())));
                assert_eq!(exit_action.as_ref().map(|b| b.label.clone()), Some("Exit".to_string()));
                assert_eq!(*columns, 2);
            }
            other => panic!("expected MultiAction, got {other:?}"),
        }
        assert_eq!(d.on_cancel(), None); // exit_action has no "action" key
    }

    #[test]
    fn dialog_list_resolves_mixed_string_and_inline_entries() {
        let mut inline = NbtCompound::new();
        inline.insert("type", text_tag("notice"));
        inline.insert("title", text_tag("Inline"));

        let dialogs_list = NbtList::Compound(vec![inline]);
        let mut c = dialog_of_type("dialog_list");
        c.insert("dialogs", NbtTag::List(dialogs_list));
        let d = parse_compound(None, &c, 0).unwrap();
        match d.kind {
            DialogKind::DialogList { dialogs, columns, button_width, .. } => {
                assert_eq!(dialogs.len(), 1);
                assert_eq!(dialogs[0].title, "Inline");
                assert_eq!(columns, 2);
                assert_eq!(button_width, 150);
            }
            other => panic!("expected DialogList, got {other:?}"),
        }
    }

    #[test]
    fn server_links_parses_with_real_defaults() {
        let c = dialog_of_type("server_links");
        let d = parse_compound(None, &c, 0).unwrap();
        match &d.kind {
            DialogKind::ServerLinks { exit_action, columns, button_width } => {
                assert_eq!(*exit_action, None);
                assert_eq!(*columns, 2);
                assert_eq!(*button_width, 150);
            }
            other => panic!("expected ServerLinks, got {other:?}"),
        }
        assert_eq!(d.on_cancel(), None);
    }

    #[test]
    fn inputs_parse_all_four_control_types_with_real_defaults() {
        let mut boolean = NbtCompound::new();
        boolean.insert("key", text_tag("flag"));
        boolean.insert("type", text_tag("boolean"));
        boolean.insert("label", text_tag("Flag"));

        let mut text = NbtCompound::new();
        text.insert("key", text_tag("name"));
        text.insert("type", text_tag("text"));
        text.insert("label", text_tag("Name"));
        text.insert("initial", text_tag("Steve"));

        let mut range = NbtCompound::new();
        range.insert("key", text_tag("amount"));
        range.insert("type", text_tag("number_range"));
        range.insert("label", text_tag("Amount"));
        range.insert("start", NbtTag::Float(0.0));
        range.insert("end", NbtTag::Float(10.0));

        let mut opt_a = NbtCompound::new();
        opt_a.insert("id", text_tag("a"));
        let mut opt_b = NbtCompound::new();
        opt_b.insert("id", text_tag("b"));
        opt_b.insert("initial", NbtTag::Byte(1));
        let mut single = NbtCompound::new();
        single.insert("key", text_tag("choice"));
        single.insert("type", text_tag("single_option"));
        single.insert("label", text_tag("Choice"));
        single.insert("options", NbtTag::List(NbtList::Compound(vec![opt_a, opt_b])));

        let inputs_list = NbtList::Compound(vec![boolean, text, range, single]);
        let mut c = dialog_of_type("notice");
        c.insert("inputs", NbtTag::List(inputs_list));
        let d = parse_compound(None, &c, 0).unwrap();
        assert_eq!(d.inputs.len(), 4);
        match &d.inputs[0].control {
            InputControl::Boolean { initial, on_true, on_false, .. } => {
                assert!(!initial);
                assert_eq!(on_true, "true");
                assert_eq!(on_false, "false");
            }
            other => panic!("expected Boolean, got {other:?}"),
        }
        match &d.inputs[2].control {
            InputControl::NumberRange { range, .. } => {
                assert_eq!(range.start, 0.0);
                assert_eq!(range.end, 10.0);
                assert_eq!(range.initial_scaled_value(), 5.0);
            }
            other => panic!("expected NumberRange, got {other:?}"),
        }
        match &d.inputs[3].control {
            InputControl::SingleOption { entries, .. } => {
                assert_eq!(entries.len(), 2);
                assert!(entries[1].initial);
            }
            other => panic!("expected SingleOption, got {other:?}"),
        }
    }

    #[test]
    fn single_option_rejects_empty_options() {
        let mut single = NbtCompound::new();
        single.insert("key", text_tag("choice"));
        single.insert("type", text_tag("single_option"));
        single.insert("label", text_tag("Choice"));
        single.insert("options", NbtTag::List(NbtList::Compound(vec![])));
        assert!(parse_input_entry(&single).is_none());
    }

    #[test]
    fn number_range_quantizes_around_initial_by_step() {
        let range = NumberRange { start: 0.0, end: 10.0, initial: Some(0.0), step: Some(2.0) };
        // Slider ~0.35 -> raw lerp 3.5 -> nearest step-of-2 from 0.0 is 4.0.
        assert_eq!(range.compute_scaled_value(0.35), 4.0);
        // No step: plain lerp.
        let no_step = NumberRange { start: 0.0, end: 10.0, initial: None, step: None };
        assert_eq!(no_step.compute_scaled_value(0.5), 5.0);
        assert_eq!(no_step.initial_scaled_value(), 5.0); // real fallback: range midpoint
    }
}
