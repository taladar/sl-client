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
//! - a `SlashCommandInvoked` — when the line is `/<name> …` and `<name>` (a
//!   **non-numeric** token) is in the `SlashCommands` registry, so the
//!   registrant handles it.
//!
//! The nearby-chat bar and the conversations floater are the intended live
//! consumers (each its own follow-up); both spawn this widget and wire its output.
//!
//! # The parse (`classify_line`)
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
use bevy::ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide};
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

/// How close to the window edge the volume dropdown may go, in logical pixels —
/// the same margin [`sl_viewer_ui_widgets::ui_combo`]'s dropdown keeps.
const POPOVER_WINDOW_MARGIN: f32 = 4.0;

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
/// a `SlashCommandInvoked` instead of chat; an unregistered one is said
/// verbatim.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct SlashCommands {
    /// The registered command names, lower-cased.
    names: HashSet<String>,
}

impl SlashCommands {
    /// Register `name` (case-insensitively) as a slash command, so `/name …` in a
    /// local-chat input routes to a `SlashCommandInvoked` rather than chat.
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
pub struct LocalChatSubmit {
    /// The field the line came from.
    pub field: Entity,
    /// The channel to say on (`0` for local chat).
    pub channel: ChatChannel,
    /// The wire chat type (volume) to say at.
    pub chat_type: ChatType,
    /// The message text.
    pub message: String,
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
pub struct LocalChatInputHandle {
    /// The chat box (from [`crate::chat_input`]).
    pub(crate) container: Entity,
    /// The inner [`bevy::text::EditableText`] field. Used by the nearby-chat bar
    /// ([`crate::nearby_chat_bar`]) to focus it and read its value; the specimen
    /// uses only `container`.
    pub field: Entity,
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
pub fn spawn_local_chat_input(
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
    // The dropdown panel, hidden until the button is clicked, and placed by
    // `bevy_ui_widgets`' popover positioner rather than by hand: `Top` is the
    // side it wants (the chat bar usually sits at the bottom of the screen),
    // `Bottom` is the fallback the positioner falls through to when there is no
    // room above — a chat bar docked at the top, or this widget reused in a
    // panel anywhere else. Hand-positioning it at `bottom: 100%` had no such
    // fallback and no window margin, and laid three of the four rows out above
    // the top edge of the window (`viewer-chat-volume-dropdown-opens-off-screen`).
    //
    // `align: End` reproduces the old `right: 0` — the panel's right edge lines
    // up with the button's.
    let dropdown = commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                min_width: Val::Px(72.0),
                border: UiRect::all(Val::Px(1.0)),
                padding: UiRect::all(Val::Px(2.0)),
                ..column(Val::Px(0.0))
            },
            Popover {
                positions: vec![
                    PopoverPlacement {
                        side: PopoverSide::Top,
                        align: PopoverAlign::End,
                        gap: 0.0,
                    },
                    PopoverPlacement {
                        side: PopoverSide::Bottom,
                        align: PopoverAlign::End,
                        gap: 0.0,
                    },
                ],
                window_margin: POPOVER_WINDOW_MARGIN,
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
pub struct LocalChatInputPlugin;

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
/// or a `SlashCommandInvoked`, resolving the volume from the select box and the
/// `Enter` modifiers and classifying the line against the `SlashCommands`
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
pub fn spawn_local_chat_input_specimen(
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
    use super::{ChatAction, ChatSayVolume, classify_line, resolve_volume, spawn_local_chat_input};
    use crate::chat_input::ChatInputSpec;
    use crate::ui::{UiRoot, UiScaffoldSystems};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ChatChannel, ChatType};
    use sl_viewer_testkit::interact::{self, InteractionTest};
    use sl_viewer_testkit::{LayoutTest, TestError, box_of, settle};

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

    /// The viewport both placement cases are measured in, in logical pixels.
    const VIEWPORT: UVec2 = UVec2::new(800, 600);

    /// How tall the bar hosting the chat input is, in logical pixels — enough
    /// for the field, and small enough that parking it at either end of an
    /// 800×600 window leaves room on exactly one side of the volume button.
    const BAR_HEIGHT: f32 = 40.0;

    /// The tolerance the side assertions allow, in pixels: the volume button's
    /// own border.
    ///
    /// Not slack. Absolute positioning is measured against the anchor's
    /// **padding** box, so the positioner starts the panel at the inside of the
    /// button's 1 px border rather than the outside — the panel overlaps the
    /// border line by exactly that and nothing else. Written as the button's
    /// border because that is what it is, so a thicker border moves this
    /// deliberately rather than silently widening a fudge factor.
    const ANCHOR_BORDER: f32 = 1.0;

    /// Open the volume dropdown with the chat bar parked `top` logical pixels
    /// down the window, and answer with the button's and the panel's boxes.
    ///
    /// The bar is absolutely positioned rather than laid out in flow because the
    /// whole question is *where on the screen the anchor is*: the same widget
    /// against the bottom edge and against the top edge must reach opposite
    /// answers, and nothing else about it changes between the two.
    fn open_dropdown_with_bar_at(top: f32) -> Result<(Rect, Rect), TestError> {
        let mut app =
            InteractionTest::over(LayoutTest::new().with_viewport(VIEWPORT.x, VIEWPORT.y)).build();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_message::<crate::emoji_picker::OpenEmojiPicker>()
            .add_plugins((
                crate::emoji_complete::ColonCompletePlugin,
                crate::chat_input::ChatInputPlugin,
                super::LocalChatInputPlugin,
            ))
            .add_systems(
                Startup,
                (move |mut commands: Commands, root: Res<UiRoot>| {
                    let bar = commands
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(0.0),
                                top: Val::Px(top),
                                width: Val::Percent(100.0),
                                height: Val::Px(BAR_HEIGHT),
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            Name::new("host-bar"),
                            ChildOf(root.0),
                        ))
                        .id();
                    spawn_local_chat_input(
                        &mut commands,
                        bar,
                        &ChatInputSpec::new("local-chat-input"),
                    );
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
        settle(&mut app);

        interact::click_node(&mut app, "local-chat-volume-button")?;
        let button = box_of(&mut app, "local-chat-volume-button")
            .ok_or("the volume button is not a laid-out node")?;
        let dropdown = box_of(&mut app, "local-chat-volume-dropdown")
            .ok_or("the volume dropdown is not a laid-out node")?;
        assert!(
            dropdown.size().y > 0.0,
            "the dropdown did not open, so its placement says nothing"
        );
        Ok((button, dropdown))
    }

    /// **The dropdown opens on whichever side of the button has room, and never
    /// off the window.**
    ///
    /// `viewer-chat-volume-dropdown-opens-off-screen`: the panel used to be
    /// hand-positioned at `bottom: 100%`, which reads "entirely above my anchor"
    /// with no fallback and no window margin. That is right only while the chat
    /// bar happens to sit at the bottom of the screen — the same widget in a bar
    /// docked at the top laid all six of its rows out at negative Y, where three
    /// of the four options cannot be clicked at all.
    ///
    /// Both ends of the window are driven, because a single case cannot tell a
    /// fallback from a hard-coded side: parked at the bottom the panel must go
    /// **up** (its preferred placement), parked at the top it must go **down**,
    /// and in both cases stay inside the viewport.
    #[test]
    fn the_volume_dropdown_opens_on_the_side_with_room() -> Result<(), TestError> {
        let window = Rect {
            min: Vec2::ZERO,
            max: VIEWPORT.as_vec2(),
        };

        // Against the bottom edge, as the nearby-chat bar sits: room above only.
        let (button, dropdown) = open_dropdown_with_bar_at(VIEWPORT.as_vec2().y - BAR_HEIGHT)?;
        assert!(
            dropdown.max.y <= button.min.y + ANCHOR_BORDER,
            "with room only above it, the dropdown opened at {dropdown:?} rather than above the \
             button at {button:?}"
        );
        assert_eq!(
            dropdown.intersect(window),
            dropdown,
            "the dropdown left the {window:?} viewport"
        );

        // Against the top edge — a docked chat bar, a floater, the gallery card:
        // no room above, so the fallback placement must be taken.
        let (button, dropdown) = open_dropdown_with_bar_at(0.0)?;
        assert!(
            dropdown.min.y >= button.max.y - ANCHOR_BORDER,
            "with no room above it, the dropdown opened at {dropdown:?} rather than below the \
             button at {button:?}"
        );
        assert_eq!(
            dropdown.intersect(window),
            dropdown,
            "the dropdown left the {window:?} viewport"
        );
        Ok(())
    }
}
