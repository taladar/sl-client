//! The **region restart countdown**: one card that counts down to a restart of
//! the region the agent is in, whichever grid announced it.
//!
//! Second Life names its notice (`RegionRestartMinutes` / `RegionRestartSeconds`
//! keyed alerts) and OpenSim spells it out (`/Region will restart in 30
//! seconds`, an agent alert). `sl-proto` reads both into one
//! [`RegionRestartNotice`] ([`SlSessionEvent::RegionRestart`]), so this host
//! needs no grid of its own: a notice opens the card, or resets the one already
//! up to the new count, as the reference's `LLFloaterRegionRestarting::
//! updateTime` does; the card then ticks down once a second by itself.
//!
//! The card **sticks** ([`NotificationKind::Alert`]) and goes when the user
//! closes it, when the restart is cancelled, when the agent leaves the region
//! (the reference closes its floater on a region change) or when the session
//! ends. The alert the notice arrived in is not also shown: the countdown is
//! the reference's default way of showing it (`FSUseNewRegionRestartNotification`)
//! — see [`is_restart_notice`], which the alert host consults. A cancellation
//! is shown as well as obeyed.

use std::time::Duration;

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;

use sl_client_bevy::{
    AlertInfo, RegionRestartNotice, SlCurrentRegion, SlEvent, SlRegionIdentity, SlSessionEvent,
};

use crate::i18n::{TransArgs, Translator};
use crate::notification_host::{
    NotificationChannelRoot, ResolveNotification, ToastSpec, adopt_toast,
};
use crate::notifications::{NotificationKind, NotificationManager, NotificationPriority};
use crate::skin_palette::SkinPalette;
use crate::ui::{column, row};
use crate::ui_font::UiFont;
use sl_viewer_ui_core::glyph;
use sl_viewer_ui_core::semantic::{Role, Semantic};
use sl_viewer_ui_core::skin::{BUTTON_CLASS, role_class};

/// The template name the card reports under in the toast bookkeeping — the
/// reference's `region_restarting` floater, which this card stands in for.
const RESTART_TEMPLATE: &str = "RegionRestarting";

/// The countdown line's name, which automation finds it by.
pub const COUNTDOWN_NAME: &str = "region-restart-countdown";

/// The skin class a card wears (`.sk-toast`).
const CARD_CLASS: &str = "sk-toast";

/// The skin class the card's text wears (`.sk-toast-text`).
const TEXT_CLASS: &str = "sk-toast-text";

/// A card's widest allowed width, in logical pixels.
const CARD_MAX_WIDTH: f32 = 360.0;

/// A card's inner padding, in logical pixels.
const CARD_PADDING: f32 = 10.0;

/// A card's border width, in logical pixels — the restart accent is painted on
/// it.
const CARD_BORDER: f32 = 2.0;

/// The gap between a card's stacked rows, in logical pixels.
const CARD_ROW_GAP: f32 = 6.0;

/// The card body text size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// The heading and the countdown's text size, in logical pixels.
const HEADING_FONT_SIZE: f32 = 16.0;

/// The width bound of a text line, inside the padding and border.
const TEXT_MAX_WIDTH: f32 = CARD_MAX_WIDTH - 2.0 * CARD_PADDING - 2.0 * CARD_BORDER;

/// A card's fallback background — the skin's `.sk-toast` overrides it.
const CARD_BACKGROUND: Color = Color::srgba(0.10, 0.12, 0.16, 0.98);

/// The card's fallback text colour — the skin's `.sk-toast-text` overrides it.
const TEXT_COLOR: Color = SkinPalette::FALLBACK.text_primary;

/// The restart accent: a warning red, on the border and the countdown.
const ACCENT_COLOR: Color = Color::srgb(0.90, 0.36, 0.30);

/// The plugin: opens, resets, ticks and closes the countdown card.
#[derive(Debug, Clone, Copy, Default)]
pub struct RegionRestartPlugin;

impl Plugin for RegionRestartPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RestartCountdown>()
            .add_systems(Update, (ingest_restart_notices, tick_countdown).chain());
    }
}

/// The countdown card on screen, if one is.
#[derive(Resource, Debug, Default)]
struct RestartCountdown {
    /// The card, while it is up.
    card: Option<CountdownCard>,
}

/// One countdown card's entities and its deadline.
#[derive(Debug, Clone, Copy)]
struct CountdownCard {
    /// The card root (the toast).
    root: Entity,
    /// The countdown line, rewritten each second.
    countdown: Entity,
    /// When the region restarts, on the app clock.
    deadline: Duration,
    /// The whole seconds the line last showed.
    shown: u64,
}

/// Whether an alert is a restart notice the countdown shows in its stead —
/// for the alert host, which then leaves it alone.
#[must_use]
pub fn is_restart_notice(message: &str, alert_info: &[AlertInfo]) -> bool {
    // A cancellation still shows as the system message it is, as the
    // reference shows it while closing its countdown.
    matches!(
        RegionRestartNotice::from_alert(message, alert_info),
        Some(RegionRestartNotice::Scheduled { .. })
    )
}

/// The seconds left before `deadline`, rounded up, at `now`.
const fn seconds_left(deadline: Duration, now: Duration) -> u64 {
    let left = deadline.saturating_sub(now);
    let whole = left.as_secs();
    if left.subsec_nanos() > 0 {
        whole.saturating_add(1)
    } else {
        whole
    }
}

/// The countdown line for `seconds`.
fn countdown_text(translator: &Translator, seconds: u64) -> String {
    translator.format(
        "region-restart-countdown",
        &TransArgs::new().text("seconds", &seconds.to_string()),
    )
}

/// Read the session's restart notices and the events that end a countdown.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system: each parameter is one resource or query it reads"
)]
fn ingest_restart_notices(
    mut events: MessageReader<SlEvent>,
    channel: Option<Res<NotificationChannelRoot>>,
    mut manager: ResMut<NotificationManager>,
    mut countdown: ResMut<RestartCountdown>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    roots: Query<(), With<Node>>,
    translator: Translator,
    time: Res<Time>,
    mut resolves: MessageWriter<ResolveNotification>,
    mut commands: Commands,
) {
    // A card the user closed is gone from the world; forget it.
    if countdown
        .card
        .is_some_and(|card| roots.get(card.root).is_err())
    {
        countdown.card = None;
    }
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::RegionRestart(RegionRestartNotice::Scheduled {
                seconds,
                region_name,
            }) => {
                let deadline = time
                    .elapsed()
                    .saturating_add(Duration::from_secs(u64::from(*seconds)));
                if let Some(card) = countdown.card.as_mut() {
                    card.deadline = deadline;
                    card.shown = u64::MAX;
                    continue;
                }
                let Some(channel) = channel.as_deref() else {
                    continue;
                };
                let name = region_name.clone().unwrap_or_else(|| {
                    regions
                        .iter()
                        .next()
                        .and_then(|region| region.0.sim_name.as_ref())
                        .map_or_else(
                            || translator.get("region-restart-unknown-region"),
                            ToString::to_string,
                        )
                });
                countdown.card = Some(spawn_countdown_card(
                    &mut commands,
                    channel,
                    &mut manager,
                    &translator,
                    &name,
                    deadline,
                    u64::from(*seconds),
                ));
            }
            SlSessionEvent::RegionRestart(RegionRestartNotice::Cancelled)
            | SlSessionEvent::RegionChanged { .. }
            | SlSessionEvent::Disconnected(_)
            | SlSessionEvent::LoggedOut => {
                if let Some(card) = countdown.card.take() {
                    resolves.write(ResolveNotification {
                        toast: card.root,
                        button: None,
                    });
                }
            }
            _other => {}
        }
    }
}

/// Rewrite the countdown line whenever the whole second it shows moves.
fn tick_countdown(
    mut countdown: ResMut<RestartCountdown>,
    translator: Translator,
    time: Res<Time>,
    mut texts: Query<&mut Text>,
) {
    let Some(card) = countdown.card.as_mut() else {
        return;
    };
    let left = seconds_left(card.deadline, time.elapsed());
    if left == card.shown {
        return;
    }
    card.shown = left;
    if let Ok(mut text) = texts.get_mut(card.countdown) {
        *text = Text::new(countdown_text(&translator, left));
    }
}

/// Build the card, adopt it into the toast channel and wire its close box.
fn spawn_countdown_card(
    commands: &mut Commands,
    channel: &NotificationChannelRoot,
    manager: &mut NotificationManager,
    translator: &Translator,
    region_name: &str,
    deadline: Duration,
    seconds: u64,
) -> CountdownCard {
    let root = commands
        .spawn((
            Node {
                max_width: Val::Px(CARD_MAX_WIDTH),
                padding: UiRect::all(Val::Px(CARD_PADDING)),
                border: UiRect::all(Val::Px(CARD_BORDER)),
                ..column(Val::Px(CARD_ROW_GAP))
            },
            BackgroundColor(CARD_BACKGROUND),
            BorderColor::all(ACCENT_COLOR),
            ClassList::new_with_classes([CARD_CLASS]),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("region-restart-card"),
        ))
        .id();
    let close = spawn_close_button(commands, root);
    let heading = translator.get("region-restart-heading");
    let body = translator.format(
        "region-restart-body",
        &TransArgs::new().text("name", region_name),
    );
    spawn_text(commands, root, &heading, HEADING_FONT_SIZE, TEXT_COLOR);
    spawn_text(commands, root, &body, FONT_SIZE, TEXT_COLOR);
    let countdown = spawn_text(
        commands,
        root,
        &countdown_text(translator, seconds),
        HEADING_FONT_SIZE,
        ACCENT_COLOR,
    );
    commands.entity(countdown).insert(Name::new(COUNTDOWN_NAME));
    let _id = adopt_toast(
        commands,
        manager,
        channel,
        root,
        ToastSpec {
            kind: NotificationKind::Alert,
            priority: NotificationPriority::High,
            template: RESTART_TEMPLATE,
            default_button: None,
            history_body: body,
            buttons: Vec::new(),
        },
    );
    commands.entity(close).observe(
        move |_activate: On<Activate>, mut resolves: MessageWriter<ResolveNotification>| {
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );
    CountdownCard {
        root,
        countdown,
        deadline,
        shown: seconds,
    }
}

/// The close (×) button in a top-trailing row; its box is returned to wire.
fn spawn_close_button(commands: &mut Commands, card: Entity) -> Entity {
    let close_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::End,
                ..row(Val::ZERO)
            },
            Name::new("region-restart-close-row"),
            ChildOf(card),
        ))
        .id();
    commands
        .spawn((
            Button,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                ..default()
            },
            ClassList::new_with_classes([BUTTON_CLASS]),
            Semantic::new(Role::Button).name_key("notification-button-close"),
            Name::new("region-restart-close"),
            ChildOf(close_row),
        ))
        .with_child((
            glyph::glyph_host(
                glyph::DISMISS,
                UiFont::Sans.at(FONT_SIZE),
                role_class(TEXT_COLOR),
            ),
            TextColor(TEXT_COLOR),
        ))
        .id()
}

/// A width-bounded text line in its own decoration-free box, so it wraps; the
/// text entity is returned to rewrite.
fn spawn_text(
    commands: &mut Commands,
    parent: Entity,
    text: &str,
    font_size: f32,
    color: Color,
) -> Entity {
    let box_entity = commands
        .spawn((
            Node {
                max_width: Val::Px(TEXT_MAX_WIDTH),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    commands
        .spawn((
            Text::new(text.to_owned()),
            UiFont::Sans.at(font_size),
            TextColor(color),
            ClassList::new_with_classes([TEXT_CLASS]),
            Pickable::IGNORE,
            ChildOf(box_entity),
        ))
        .id()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_client_bevy::AlertInfo;

    use super::{is_restart_notice, seconds_left};

    /// The count rounds up, so a card shows its full count on the frame it
    /// opens and reaches zero only at the deadline.
    #[test]
    fn seconds_left_rounds_up() {
        let deadline = Duration::from_secs(30);
        assert_eq!(seconds_left(deadline, Duration::ZERO), 30);
        assert_eq!(seconds_left(deadline, Duration::from_millis(100)), 30);
        assert_eq!(seconds_left(deadline, Duration::from_millis(29_001)), 1);
        assert_eq!(seconds_left(deadline, Duration::from_secs(30)), 0);
        assert_eq!(seconds_left(deadline, Duration::from_secs(40)), 0);
    }

    /// Both grids' notices are the countdown's; anything else is not.
    #[test]
    fn restart_notices_are_recognised() {
        assert!(is_restart_notice("/Region will restart in 30 seconds", &[]));
        let keyed = AlertInfo {
            message: "RegionRestartSeconds".to_owned(),
            extra_params: "<llsd><map><key>SECONDS</key><integer>30</integer></map></llsd>"
                .to_owned(),
        };
        assert!(is_restart_notice("", &[keyed]));
        assert!(!is_restart_notice("/estate census message", &[]));
        assert!(!is_restart_notice("/Region restart cancelled.", &[]));
    }
}
