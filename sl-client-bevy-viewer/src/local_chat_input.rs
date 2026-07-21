//! The reusable **local-chat-input widget** (`viewer-chat-channel-and-commands`):
//! the [`crate::chat_input`] chat box plus the local-chat behaviours — a
//! **Whisper / Say / Shout** select box beside the emoji button, `/N …` **channel
//! routing**, `Shift+Enter` / `Ctrl+Enter` → **whisper / shout**, and a general
//! **`/command` registry** other parts of the viewer register into.
//!
//! # Still just a widget
//!
//! Like [`crate::chat_input`] it reaches no session (per [`crate::ui_element`]).
//! It interprets a [`crate::chat_input::ChatInputSubmit`] and emits **one of two**
//! structured outputs:
//!
//! - a [`LocalChatSubmit`] — the resolved channel, chat type (volume) and message,
//!   which a live consumer maps to `Command::Chat`; or
//! - a [`SlashCommandInvoked`] — when the line is `/<name> …` and `<name>` (a
//!   **non-numeric** token) is in the [`SlashCommands`] registry, so the
//!   registrant handles it.
//!
//! The nearby-chat bar and the conversations floater are the intended live
//! consumers (each its own follow-up); both spawn this widget and wire its output.
//!
//! # The parse ([`classify_line`])
//!
//! - `/<number> rest` → channel `number`, **Normal** type (whisper/shout apply
//!   only to channel 0), message `rest`.
//! - `/<name> rest`, `<name>` non-numeric and **registered** → a command.
//! - `/<name> …`, `<name>` **not** registered (or a bare `/`) → said **verbatim**
//!   on channel 0 (the reference says an unrecognised slash line as-is, which is
//!   also how `/me …` reaches the sim to be rendered as an emote).
//! - anything else → channel 0 at the resolved volume.
//!
//! Volume is the select box's choice, **overridden** by the `Enter` modifiers
//! (`Ctrl` → shout, `Shift` → whisper), matching Firestorm's `FSUseCtrlShout` /
//! `FSUseShiftWhisper`.
//!
//! Reference (Firestorm, read-only): `llchatbar` channel parsing, `LLChat`
//! chat-type handling, `fsnearbychatcontrol` Enter modifiers.

use std::collections::HashSet;

use bevy::prelude::*;
use sl_client_bevy::{ChatChannel, ChatType};

use crate::chat_input::{ChatInputHandle, ChatInputSpec, ChatInputSubmit, spawn_chat_input};
use crate::ui::column;
use crate::ui_font::UiFont;

/// The public local-chat channel (`0`).
const PUBLIC_CHANNEL: ChatChannel = ChatChannel(0);

/// The select box's font size, in logical pixels.
const SELECT_FONT_SIZE: f32 = 13.0;

/// The select box border.
const SELECT_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The select box / option text colour.
const SELECT_TEXT_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dropdown option's resting background.
const OPTION_BACKGROUND: Color = Color::NONE;

/// The current volume's option background (and the button tint) — so the active
/// choice reads at a glance.
const OPTION_ACTIVE_BACKGROUND: Color = Color::srgb(0.22, 0.40, 0.60);

/// The dropdown panel background.
const DROPDOWN_BACKGROUND: Color = Color::srgba(0.10, 0.12, 0.16, 0.98);

/// The three chat volumes the select box offers — the local-chat range, which the
/// `/N` channel form and the command form both bypass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatSayVolume {
    /// Whisper — reduced range.
    Whisper,
    /// Say — the default range.
    Say,
    /// Shout — extended range.
    Shout,
}

impl ChatSayVolume {
    /// The three volumes, in the select box's order.
    const ALL: [Self; 3] = [Self::Whisper, Self::Say, Self::Shout];

    /// This volume's display label.
    const fn label(self) -> &'static str {
        match self {
            Self::Whisper => "Whisper",
            Self::Say => "Say",
            Self::Shout => "Shout",
        }
    }

    /// The wire chat type this volume sends as.
    const fn chat_type(self) -> ChatType {
        match self {
            Self::Whisper => ChatType::Whisper,
            Self::Say => ChatType::Normal,
            Self::Shout => ChatType::Shout,
        }
    }
}

/// The local-chat state carried on the field: its current select-box volume.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct LocalChatInput {
    /// The volume the select box currently shows — the default for a plain
    /// `Enter`, overridden by the `Shift` / `Ctrl` modifiers.
    volume: ChatSayVolume,
}

/// The volume select button, naming its field and its label node so the label can
/// track the chosen volume. The dropdown it toggles is captured by its own press
/// observer.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct VolumeButton {
    /// The field whose volume this button shows.
    field: Entity,
    /// The button's label text node.
    label: Entity,
}

/// The dropdown panel of volume options.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct VolumeDropdown;

/// One volume option row in the dropdown.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct VolumeOption {
    /// The field this option sets the volume of.
    field: Entity,
    /// The volume this option selects.
    volume: ChatSayVolume,
}

/// The registry of **non-numeric** `/command` names other parts of the viewer
/// claim. A local-chat line `/<name> …` whose `<name>` is registered here becomes
/// a [`SlashCommandInvoked`] instead of chat; an unregistered one is said
/// verbatim.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct SlashCommands {
    /// The registered command names, lower-cased.
    names: HashSet<String>,
}

impl SlashCommands {
    /// Register `name` (case-insensitively) as a slash command, so `/name …` in a
    /// local-chat input routes to a [`SlashCommandInvoked`] rather than chat.
    #[expect(
        dead_code,
        reason = "the registration API for other parts of the viewer; its callers are the \
                  follow-up consumers (the nearby-chat bar's own commands, gestures) — no widget \
                  registers a command itself"
    )]
    pub(crate) fn register(&mut self, name: &str) {
        self.names.insert(name.to_ascii_lowercase());
    }

    /// Whether `name` (already lower-cased) is a registered command.
    fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}

/// A resolved local-chat line to say: the channel, the wire chat type, and the
/// message. A live consumer maps this to `Command::Chat` (the nearby-chat bar,
/// [`crate::nearby_chat_bar`], does).
#[derive(Message, Debug, Clone)]
pub(crate) struct LocalChatSubmit {
    /// The field the line came from.
    pub(crate) field: Entity,
    /// The channel to say on (`0` for local chat).
    pub(crate) channel: ChatChannel,
    /// The wire chat type (volume) to say at.
    pub(crate) chat_type: ChatType,
    /// The message text.
    pub(crate) message: String,
}

/// An invoked `/command`: the field, the command name (lower-cased) and the
/// argument tail. A registrant reads this, filtering on [`name`](Self::name).
#[derive(Message, Debug, Clone)]
#[expect(
    dead_code,
    reason = "the widget's published output; its fields are read by the follow-up registrants that \
              claim a command name via SlashCommands::register"
)]
pub(crate) struct SlashCommandInvoked {
    /// The field the command came from.
    pub(crate) field: Entity,
    /// The command name, lower-cased and without the leading `/`.
    pub(crate) name: String,
    /// The argument tail after the command name (leading space trimmed).
    pub(crate) args: String,
}

/// What [`spawn_local_chat_input`] hands back: the chat box, the inner field, and
/// the volume select button.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalChatInputHandle {
    /// The chat box (from [`crate::chat_input`]).
    pub(crate) container: Entity,
    /// The inner [`bevy::text::EditableText`] field. Used by the nearby-chat bar
    /// ([`crate::nearby_chat_bar`]) to focus it and read its value; the specimen
    /// uses only [`container`](Self::container).
    pub(crate) field: Entity,
}

// ---------------------------------------------------------------------------
// Pure core — the line parse and the modifier resolution, unit-tested.
// ---------------------------------------------------------------------------

/// What a submitted local-chat line resolves to: chat to say, or a registered
/// command to dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChatAction {
    /// Say `message` on `channel` at `chat_type`.
    Chat {
        /// The channel to say on.
        channel: ChatChannel,
        /// The wire chat type (volume).
        chat_type: ChatType,
        /// The message text.
        message: String,
    },
    /// Dispatch the registered command `name` with `args`.
    Command {
        /// The command name, lower-cased.
        name: String,
        /// The argument tail.
        args: String,
    },
}

/// The volume a line is said at: the select box's `base`, overridden by the
/// `Enter` modifiers — `Ctrl` (only) → shout, `Shift` (only) → whisper (the
/// reference's `FSUseCtrlShout` / `FSUseShiftWhisper`). Both or neither leave
/// `base`.
const fn resolve_volume(base: ChatSayVolume, shift: bool, ctrl: bool) -> ChatSayVolume {
    match (shift, ctrl) {
        (false, true) => ChatSayVolume::Shout,
        (true, false) => ChatSayVolume::Whisper,
        _both_or_neither => base,
    }
}

/// Classify a submitted line into a [`ChatAction`], given the resolved `volume`
/// and a predicate telling whether a `/name` token is a registered command.
///
/// See the [module docs](self) for the rules. `is_command` receives the
/// lower-cased token.
fn classify_line(
    text: &str,
    volume: ChatSayVolume,
    is_command: impl Fn(&str) -> bool,
) -> ChatAction {
    if let Some(rest) = text.strip_prefix('/') {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let token = parts.next().unwrap_or("");
        let args = parts.next().unwrap_or("").trim_start();
        if let Ok(channel) = token.parse::<i32>() {
            // Channel chat is always Normal type — whisper / shout are channel-0.
            return ChatAction::Chat {
                channel: ChatChannel(channel),
                chat_type: ChatType::Normal,
                message: args.to_owned(),
            };
        }
        let name = token.to_ascii_lowercase();
        if !name.is_empty() && is_command(&name) {
            return ChatAction::Command {
                name,
                args: args.to_owned(),
            };
        }
        // A bare `/` or an unregistered `/word` is said verbatim (this is how
        // `/me …` reaches the sim to be rendered as an emote).
    }
    ChatAction::Chat {
        channel: PUBLIC_CHANNEL,
        chat_type: volume.chat_type(),
        message: text.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// Spawn a local-chat input under `parent` — a [`crate::chat_input`] with a volume
/// select box appended after its emoji button — returning the box and field.
pub(crate) fn spawn_local_chat_input(
    commands: &mut Commands,
    parent: Entity,
    spec: &ChatInputSpec,
) -> LocalChatInputHandle {
    let ChatInputHandle { container, field } = spawn_chat_input(commands, parent, spec);
    commands.entity(field).insert(LocalChatInput {
        volume: ChatSayVolume::Say,
    });
    build_volume_select(commands, container, field);
    LocalChatInputHandle { container, field }
}

/// Build the volume select box (button + dropdown) under the chat box, for
/// `field`.
fn build_volume_select(commands: &mut Commands, container: Entity, field: Entity) {
    // The dropdown panel, above the button, hidden until the button is clicked.
    let dropdown = commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                bottom: Val::Percent(100.0),
                right: Val::Px(0.0),
                min_width: Val::Px(72.0),
                border: UiRect::all(Val::Px(1.0)),
                padding: UiRect::all(Val::Px(2.0)),
                ..column(Val::Px(0.0))
            },
            BorderColor::all(SELECT_BORDER),
            BackgroundColor(DROPDOWN_BACKGROUND),
            GlobalZIndex(10_000),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            VolumeDropdown,
            Name::new("local-chat-volume-dropdown"),
        ))
        .id();
    for volume in ChatSayVolume::ALL {
        spawn_volume_option(commands, dropdown, field, volume);
    }

    // The button, showing the current volume; its own relative box anchors the
    // dropdown.
    let label = commands
        .spawn((
            Text::new(ChatSayVolume::Say.label()),
            UiFont::Sans.at(SELECT_FONT_SIZE),
            TextColor(SELECT_TEXT_COLOR),
            Pickable::IGNORE,
        ))
        .id();
    let button = commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                position_type: PositionType::Relative,
                border: UiRect::all(Val::Px(1.0)),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(SELECT_BORDER),
            BackgroundColor(OPTION_BACKGROUND),
            Pickable::default(),
            VolumeButton { field, label },
            Name::new("local-chat-volume-button"),
            ChildOf(container),
        ))
        .add_child(label)
        .add_child(dropdown)
        .id();
    commands.entity(button).observe(
        move |mut press: On<Pointer<Press>>, mut nodes: Query<&mut Node, With<VolumeDropdown>>| {
            press.propagate(false);
            if press.button != PointerButton::Primary {
                return;
            }
            if let Ok(mut node) = nodes.get_mut(dropdown) {
                node.display = if node.display == Display::None {
                    Display::Flex
                } else {
                    Display::None
                };
            }
        },
    );
}

/// Spawn one volume option row in the dropdown, wiring its press to select that
/// volume and close the dropdown.
fn spawn_volume_option(
    commands: &mut Commands,
    dropdown: Entity,
    field: Entity,
    volume: ChatSayVolume,
) {
    let option = commands
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(OPTION_BACKGROUND),
            Pickable::default(),
            VolumeOption { field, volume },
            ChildOf(dropdown),
        ))
        .with_child((
            Text::new(volume.label()),
            UiFont::Sans.at(SELECT_FONT_SIZE),
            TextColor(SELECT_TEXT_COLOR),
            Pickable::IGNORE,
        ))
        .id();
    commands.entity(option).observe(
        move |mut press: On<Pointer<Press>>,
              mut inputs: Query<&mut LocalChatInput>,
              mut dropdowns: Query<&mut Node, With<VolumeDropdown>>| {
            press.propagate(false);
            if press.button != PointerButton::Primary {
                return;
            }
            if let Ok(mut input) = inputs.get_mut(field) {
                input.volume = volume;
            }
            if let Ok(mut node) = dropdowns.get_mut(dropdown) {
                node.display = Display::None;
            }
        },
    );
}

// ---------------------------------------------------------------------------
// Plugin & systems
// ---------------------------------------------------------------------------

/// The local-chat-input widget's runtime: the select-box reflection and the line
/// dispatch. Requires [`crate::chat_input::ChatInputPlugin`] (whose
/// [`ChatInputSubmit`] it reads).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LocalChatInputPlugin;

impl Plugin for LocalChatInputPlugin {
    /// Register the registry, the output messages, and the systems.
    fn build(&self, app: &mut App) {
        app.init_resource::<SlashCommands>()
            .add_message::<LocalChatSubmit>()
            .add_message::<SlashCommandInvoked>()
            .add_systems(Update, (dispatch_local_chat, reflect_volume_select));
    }
}

/// Turn each [`ChatInputSubmit`] from a local-chat field into a [`LocalChatSubmit`]
/// or a [`SlashCommandInvoked`], resolving the volume from the select box and the
/// `Enter` modifiers and classifying the line against the [`SlashCommands`]
/// registry.
fn dispatch_local_chat(
    mut submits: MessageReader<ChatInputSubmit>,
    inputs: Query<&LocalChatInput>,
    registry: Res<SlashCommands>,
    mut chat_out: MessageWriter<LocalChatSubmit>,
    mut command_out: MessageWriter<SlashCommandInvoked>,
) {
    for submit in submits.read() {
        let Ok(input) = inputs.get(submit.field) else {
            continue;
        };
        let volume = resolve_volume(input.volume, submit.shift, submit.ctrl);
        match classify_line(&submit.text, volume, |name| registry.contains(name)) {
            ChatAction::Chat {
                channel,
                chat_type,
                message,
            } => {
                chat_out.write(LocalChatSubmit {
                    field: submit.field,
                    channel,
                    chat_type,
                    message,
                });
            }
            ChatAction::Command { name, args } => {
                command_out.write(SlashCommandInvoked {
                    field: submit.field,
                    name,
                    args,
                });
            }
        }
    }
}

/// Keep each volume button's label showing its field's current volume, and
/// highlight the matching dropdown option.
fn reflect_volume_select(
    inputs: Query<&LocalChatInput>,
    buttons: Query<&VolumeButton>,
    mut texts: Query<&mut Text>,
    mut options: Query<(&VolumeOption, &mut BackgroundColor)>,
) {
    for button in &buttons {
        let Ok(input) = inputs.get(button.field) else {
            continue;
        };
        if let Ok(mut text) = texts.get_mut(button.label) {
            let wanted = input.volume.label();
            if text.0 != wanted {
                wanted.clone_into(&mut text.0);
            }
        }
    }
    for (option, mut background) in &mut options {
        let Ok(input) = inputs.get(option.field) else {
            continue;
        };
        let wanted = if input.volume == option.volume {
            OPTION_ACTIVE_BACKGROUND
        } else {
            OPTION_BACKGROUND
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

// ---------------------------------------------------------------------------
// Registry specimen
// ---------------------------------------------------------------------------

/// Spawn the **live** local-chat-input specimen for the gallery / harness: the real
/// widget, so its bar (with the volume select box) is swept and it is usable in the
/// gallery. Its runtime is inert in the harness and live in the gallery.
pub(crate) fn spawn_local_chat_input_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    spawn_local_chat_input(
        commands,
        parent,
        &ChatInputSpec {
            font_size: cx.font_size,
            ..ChatInputSpec::new("local-chat-input")
        },
    )
    .container
}

#[cfg(test)]
mod tests {
    use super::{ChatAction, ChatSayVolume, classify_line, resolve_volume};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ChatChannel, ChatType};

    /// No command is registered in these classification tests.
    fn no_commands(_name: &str) -> bool {
        false
    }

    /// A plain line is said on channel 0 at the resolved volume.
    #[test]
    fn plain_line_is_channel_zero_say() {
        assert_eq!(
            classify_line("hello world", ChatSayVolume::Say, no_commands),
            ChatAction::Chat {
                channel: ChatChannel(0),
                chat_type: ChatType::Normal,
                message: "hello world".to_owned(),
            }
        );
        // Shout volume flows through.
        assert_eq!(
            classify_line("loud", ChatSayVolume::Shout, no_commands),
            ChatAction::Chat {
                channel: ChatChannel(0),
                chat_type: ChatType::Shout,
                message: "loud".to_owned(),
            }
        );
    }

    /// `/N rest` routes to channel N as Normal type, whatever the volume; a
    /// negative channel parses too.
    #[test]
    fn channel_prefix_routes_and_ignores_volume() {
        assert_eq!(
            classify_line("/5 ping", ChatSayVolume::Shout, no_commands),
            ChatAction::Chat {
                channel: ChatChannel(5),
                chat_type: ChatType::Normal,
                message: "ping".to_owned(),
            }
        );
        assert_eq!(
            classify_line("/-2   spaced", ChatSayVolume::Say, no_commands),
            ChatAction::Chat {
                channel: ChatChannel(-2),
                chat_type: ChatType::Normal,
                message: "spaced".to_owned(),
            }
        );
    }

    /// A registered `/name` is a command; an unregistered one is said verbatim
    /// (this is how `/me …` reaches the sim).
    #[test]
    fn slash_word_is_command_only_when_registered() {
        let registered = |name: &str| name == "draw";
        assert_eq!(
            classify_line("/draw a circle", ChatSayVolume::Say, registered),
            ChatAction::Command {
                name: "draw".to_owned(),
                args: "a circle".to_owned(),
            }
        );
        // Case-insensitive command name.
        assert_eq!(
            classify_line("/DRAW x", ChatSayVolume::Say, registered),
            ChatAction::Command {
                name: "draw".to_owned(),
                args: "x".to_owned(),
            }
        );
        // Unregistered: said verbatim, slash and all.
        assert_eq!(
            classify_line("/me waves", ChatSayVolume::Say, no_commands),
            ChatAction::Chat {
                channel: ChatChannel(0),
                chat_type: ChatType::Normal,
                message: "/me waves".to_owned(),
            }
        );
    }

    /// The `Enter` modifiers override the base volume: Ctrl → shout, Shift →
    /// whisper, both / neither → the base.
    #[test]
    fn modifiers_override_the_base_volume() {
        assert_eq!(
            resolve_volume(ChatSayVolume::Say, false, true),
            ChatSayVolume::Shout
        );
        assert_eq!(
            resolve_volume(ChatSayVolume::Say, true, false),
            ChatSayVolume::Whisper
        );
        assert_eq!(
            resolve_volume(ChatSayVolume::Whisper, false, false),
            ChatSayVolume::Whisper
        );
        // Both held: no override (the reference makes that a linefeed).
        assert_eq!(
            resolve_volume(ChatSayVolume::Say, true, true),
            ChatSayVolume::Say
        );
    }
}
