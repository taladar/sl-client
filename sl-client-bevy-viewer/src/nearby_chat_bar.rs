//! The live **nearby-chat bar** (`viewer-chat-input-bar`): the always-visible
//! local-chat input that sits **above the bottom button bar**, sends what you type
//! as local chat, and drives the typing animation.
//!
//! It is the first live consumer of the reusable **local-chat-input widget**
//! ([`crate::local_chat_input`]) — all the input behaviour (the field, the emoji
//! button, the `:`-completer, the whisper/say/shout select box, `/N` channels, the
//! `/command` registry, the Shift/Ctrl+Enter volume overrides) lives in the widget;
//! this module only does the three things a *live* bar adds:
//!
//! 1. **Placement** — one widget spawned into the bottom area's upper stack
//!    ([`crate::bottom_toolbar::BottomArea::upper`]), so it always rides just above
//!    the toolbar buttons.
//! 2. **Send** — the widget's session-free [`LocalChatSubmit`] mapped to
//!    `Command::Chat` (message / channel / chat type straight through).
//! 3. **Focus & typing** — `Enter` while the **World** owns the keyboard focuses
//!    the bar (`Esc` blurs back, via [`crate::input_context`]); while the bar is
//!    focused and holds a draft, [`crate::typing::TypingState`] is driven so the
//!    own avatar plays the typing animation and neighbours see the "is typing"
//!    indicator.
//!
//! A **toggle button** on the leading end of the bottom button bar shows / hides
//! the bar (the reference's chat button); [`crate::bottom_toolbar`] owns that
//! button and flips [`NearbyChatBar::toggle`], reading [`NearbyChatBar::is_shown`]
//! for its lit state.
//!
//! # Deliberately not here
//!
//! The settings-gated **"a printable keypress in the World auto-starts local
//! chat"** affordance is [[viewer-chat-input-world-autostart]] — it belongs to this
//! bar but is its own follow-up. The **conversations floater** is a second live
//! consumer of the same widget ([[viewer-social-im-conversations]]).
//!
//! Reference (Firestorm, read-only): `fsnearbychatcontrol`, `llnearbychatbar`.

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{Command, SlCommand};

use crate::bottom_toolbar::BottomArea;
use crate::chat_input::ChatInputSpec;
use crate::input_context::world_has_keyboard;
use crate::local_chat_input::{LocalChatSubmit, spawn_local_chat_input};
use crate::typing::TypingState;
use crate::ui::row;

/// The fraction of the screen width the bar spans, from the **leading** edge — the
/// trailing half is left for the other bottom-edge controls (volume, voice, quick
/// preferences) as they land, and the exact split can be tuned then.
const BAR_WIDTH_FRACTION: f32 = 50.0;

/// The bar's least width, in logical pixels — a floor so it stays usable on a very
/// narrow window where half the screen would be too little.
const BAR_MIN_WIDTH: f32 = 320.0;

/// The bar's font size, in logical pixels.
const BAR_FONT_SIZE: f32 = 15.0;

/// The live nearby-chat bar's entities and state, published once it has spawned.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct NearbyChatBar {
    /// The chat field, so the bar's own systems (send, focus, typing) address it.
    field: Entity,
    /// The full-width wrapper node, shown / hidden by the toggle.
    wrapper: Entity,
    /// Whether the bar is currently shown (the toolbar toggle flips this).
    shown: bool,
}

impl NearbyChatBar {
    /// Whether the bar is currently shown — read by the toolbar toggle for its lit
    /// state.
    pub(crate) const fn is_shown(&self) -> bool {
        self.shown
    }

    /// Flip the bar shown / hidden — called by the toolbar toggle button.
    pub(crate) const fn toggle(&mut self) {
        self.shown = !self.shown;
    }
}

/// The plugin that owns the live nearby-chat bar: its one-time spawn once the
/// bottom area exists, and the send / focus / typing systems.
pub(crate) struct NearbyChatBarPlugin;

impl Plugin for NearbyChatBarPlugin {
    /// Wire the bar. The spawn is an `Update` system guarded to run once (the bottom
    /// area is inserted by [`crate::bottom_toolbar`] in `Startup`, so it is present
    /// from the first `Update`); the rest run every frame and are no-ops until the
    /// bar exists.
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                spawn_nearby_chat_bar,
                send_nearby_chat,
                focus_nearby_chat_on_enter.run_if(world_has_keyboard),
                drive_nearby_chat_typing,
                apply_nearby_chat_visibility,
            ),
        );
    }
}

/// Spawn the bar into the bottom area's upper stack, once — the [`Local`] latch
/// makes this a one-shot even though it lives in `Update` (so it can wait for the
/// bottom area without ordering against another plugin's `Startup`).
///
/// A full-width, **leading-aligned** wrapper holds the widget, and the box itself
/// spans [`BAR_WIDTH_FRACTION`] of the screen — so the bar starts at the leading
/// edge (mirrored under RTL, the wrapper being a [`crate::ui::row`]) and covers the
/// leading half, leaving the trailing half for the other bottom-edge controls.
fn spawn_nearby_chat_bar(
    mut commands: Commands,
    area: Option<Res<BottomArea>>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    let Some(area) = area else {
        return;
    };
    let wrapper = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::FlexStart,
                // No vertical padding: the chat box sits flush on the button bar
                // below it (the reference has no gap there).
                ..row(Val::ZERO)
            },
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("nearby-chat-bar"),
            ChildOf(area.upper),
        ))
        .id();
    let handle = spawn_local_chat_input(
        &mut commands,
        wrapper,
        &ChatInputSpec {
            font_size: BAR_FONT_SIZE,
            min_width: BAR_MIN_WIDTH,
            width: Some(Val::Percent(BAR_WIDTH_FRACTION)),
            ..ChatInputSpec::new("nearby-chat")
        },
    );
    commands.insert_resource(NearbyChatBar {
        field: handle.field,
        wrapper,
        // Shown by default, like the reference bar; the toolbar toggle hides it.
        shown: true,
    });
    *spawned = true;
}

/// Map the bar's [`LocalChatSubmit`] outputs to `Command::Chat`, so a typed line is
/// said on the grid. Filters to the bar's own field, so a second local-chat input
/// (the conversations floater) does not double-send through here.
fn send_nearby_chat(
    bar: Option<Res<NearbyChatBar>>,
    mut submits: MessageReader<LocalChatSubmit>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(bar) = bar else {
        // Drain so a pre-spawn submit is not replayed later.
        submits.clear();
        return;
    };
    for submit in submits.read() {
        if submit.field != bar.field {
            continue;
        }
        commands.write(SlCommand(Command::Chat {
            message: submit.message.clone(),
            chat_type: submit.chat_type,
            channel: submit.channel,
        }));
    }
}

/// Focus the bar on `Enter` while the World owns the keyboard, so a user starts
/// local chat by pressing `Enter` (the reference's chat-focus key). The field is
/// empty at that point, so the chat input's own `Enter`-to-send does not fire;
/// `Esc` blurs back to the World ([`crate::input_context`]).
fn focus_nearby_chat_on_enter(
    keyboard: Res<ButtonInput<KeyCode>>,
    bar: Option<ResMut<NearbyChatBar>>,
    mut focus: ResMut<InputFocus>,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    if let Some(mut bar) = bar {
        // Reveal the bar if the toggle had hidden it, then focus it.
        if !bar.shown {
            bar.shown = true;
        }
        focus.set(bar.field, FocusCause::Navigated);
    }
}

/// Reflect the bar's shown state onto its wrapper, and blur the field when it is
/// hidden while focused (so a hidden bar does not keep the keyboard).
fn apply_nearby_chat_visibility(
    bar: Option<Res<NearbyChatBar>>,
    mut nodes: Query<&mut Node>,
    mut focus: ResMut<InputFocus>,
) {
    let Some(bar) = bar else {
        return;
    };
    let wanted = if bar.shown {
        Display::Flex
    } else {
        Display::None
    };
    if let Ok(mut node) = nodes.get_mut(bar.wrapper)
        && node.display != wanted
    {
        node.display = wanted;
    }
    if !bar.shown && focus.get() == Some(bar.field) {
        focus.clear();
    }
}

/// Drive the own avatar's typing state from the bar: active while the bar is
/// focused and holds a draft, inactive otherwise — [`crate::typing`] reconciles the
/// wire edge and the local animation from it. This is the real driver the typing
/// module was built to wait for.
fn drive_nearby_chat_typing(
    bar: Option<Res<NearbyChatBar>>,
    focus: Res<InputFocus>,
    fields: Query<&EditableText>,
    mut typing: ResMut<TypingState>,
) {
    let Some(bar) = bar else {
        return;
    };
    let active = focus.get() == Some(bar.field)
        && fields
            .get(bar.field)
            .is_ok_and(|field| !field.value().to_string().trim().is_empty());
    if typing.is_active() != active {
        typing.set(active);
    }
}
