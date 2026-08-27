//! The **toast / notification host** (`viewer-ui-notification-host`): the screen
//! surface that stacks, times out, fades and dismisses the notifications the
//! catalogue ([`crate::notifications`]) declares — the shared substrate the
//! specific dialogs ([[viewer-permission-request-dialog]],
//! [[viewer-dialog-offers-invites]], [[viewer-dialog-lldialog]]) sit in.
//!
//! # What it renders
//!
//! A **channel** — an absolute container pinned to the trailing-top corner
//! (mirroring under RTL), stacking toasts with the highest-priority / newest at
//! the top. A [`NotificationKind::Tip`] or [`Notify`](NotificationKind::Notify)
//! toast **fades** on its timer ([`NotificationKind::lifetime_secs`] +
//! [`crate::notifications::TOAST_FADE_SECS`]); an
//! [`Alert`](NotificationKind::Alert) **sticks** until a button is clicked; an
//! [`AlertModal`](NotificationKind::AlertModal) is a **centred dialog over a
//! scrim** that blocks the world until answered. Each corner toast carries a
//! **close ×** for early dismissal, and only `MAX_VISIBLE_TOASTS` show at once
//! — the rest queue (paused) behind a **"N more ▸"** control that cycles them
//! into view, so a flood never fills the whole edge of the screen (the reference
//! notification well).
//!
//! # How it is driven
//!
//! Read a [`ShowNotification`] to raise one, write a [`NotificationResponse`]
//! when it is answered / expires, and read a [`DismissNotification`] to tear one
//! down early — the viewer's "emit a message, someone else acts" convention
//! ([`crate::ui_element`]). The single [`crate::notifications::NotificationManager`]
//! resource holds the id source, the `unique` dedup index and the history ring.
//!
//! The **live source** wiring ([`ingest_alert_messages`]) — the simulator's
//! `AlertMessage` / `AgentAlertMessage` stream, which nothing consumed before —
//! is added by the viewer binary, not this plugin, so the login-free gallery /
//! test harness can host the plugin without the session's `SlEvent` stream. The
//! **gallery specimen** ([`spawn_notification_specimen`]) is a static toast card
//! registered in `crate::ui_element::ELEMENTS`, so the host inherits the whole
//! layout matrix.
//!
//! # Timing is frame-time, not wall-clock
//!
//! A toast ages by [`Time::delta_secs`], never wall-clock, matching the chat
//! overlay (`crate::chat`) so a headless / manual-clock run is deterministic.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{
    Command, Diagnostic, SlCommand, SlCommandFailed, SlDiagnostic, SlEvent, SlSessionEvent,
};
use sl_settings::SettingValue;
use tracing::{debug, warn};

use crate::i18n::Translator;
use crate::notification_persist::{PersistNotification, PersistedKind};
use crate::notifications::{
    DismissNotification, NOTIFICATIONS, NOTIFICATIONS_SECTION, NotificationIgnore,
    NotificationKind, NotificationManager, NotificationPriority, NotificationRecord,
    NotificationResponse, NotificationTemplate, ShowNotification, TOAST_GAP,
    last_response_setting_name, substitute, template,
};
use crate::settings::ViewerSettings;
use crate::ui::{LogicalInset, LogicalRect, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::world_api::LocalChatNotice;

/// The element id the gallery specimen and its inert actions report under.
const NOTIFICATION_ELEMENT: &str = "notification-toast";

/// The z-order the toast channel renders at — above the floaters **and** the
/// top menu / status bar ([`crate::ui::BOTTOM_BAR_Z`]) so a toast is
/// never hidden by a window or a bar; the channel's own top inset keeps it clear
/// of the bar rather than overlapping it.
const TOAST_CHANNEL_Z: i32 = 9_500;

/// The z-order a modal alert's scrim renders at — above the toast channel, so a
/// blocking dialog sits over everything (including the corner toasts).
const MODAL_SCRIM_Z: i32 = 9_800;

/// The channel's inset from the trailing edge, in logical pixels.
const CHANNEL_INLINE_INSET: f32 = 12.0;

/// The channel's inset from the top edge, in logical pixels — enough to clear
/// the top menu / status bar so a toast stacks below it rather than under it.
const CHANNEL_BLOCK_INSET: f32 = 40.0;

/// A toast card's widest allowed width, in logical pixels — a bound, not a size,
/// so a short toast is narrow and a long one wraps here.
const TOAST_MAX_WIDTH: f32 = 360.0;

/// A toast card's inner padding, in logical pixels.
const TOAST_PADDING: f32 = 10.0;

/// A toast card's border width, in logical pixels — the meaning-bearing kind
/// accent is painted on it in Rust.
const TOAST_BORDER: f32 = 2.0;

/// The body text's width bound inside a card: the card's content width less its
/// padding and border, so the paragraph is the **sole** inline occupant of a
/// decoration-free box (the `viewer-text-node-padding-measure` constraint — see
/// [`crate::ui_element::spawn_label`]).
const TEXT_MAX_WIDTH: f32 = TOAST_MAX_WIDTH - 2.0 * TOAST_PADDING - 2.0 * TOAST_BORDER;

/// The gap between a card's stacked rows (header / buttons / ignore), in logical
/// pixels.
const CARD_ROW_GAP: f32 = 8.0;

/// The gap between a card's buttons, in logical pixels.
const BUTTON_GAP: f32 = 8.0;

/// The width the ignore checkbox + its gap claim, subtracted from the body width
/// to bound the ignore label so it wraps within the card.
const IGNORE_CHECKBOX_ALLOWANCE: f32 = 32.0;

/// The toast text size, in logical pixels.
const TOAST_FONT_SIZE: f32 = 14.0;

/// A toast card's fallback background, used when no skin is loaded — the skin's
/// `.sk-toast` (`var(--surface-bg)`) overrides it. Mostly opaque so the toast
/// reads against the world behind it.
const CARD_BACKGROUND: Color = Color::srgba(0.10, 0.12, 0.16, 0.96);

/// A toast card's fallback body text colour — the skin's `.sk-toast-text`
/// overrides it.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.93, 0.97);

/// A toast button's fallback background — the skin's `.sk-button` overrides it.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A toast button's fallback border — the skin's `.sk-button` overrides it.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// The modal scrim's translucent black, dimming the world behind a blocking
/// dialog.
const SCRIM_COLOR: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);

/// The CSS class on a toast card, so a skin recolours its surface. The
/// meaning-bearing kind accent (the border colour) is painted in Rust, one place
/// for the four kinds, so `.sk-toast` carries only the surface and corner — like
/// `.sk-toolbar-button` — and the skin does not fight the accent.
const CARD_CLASS: &str = "sk-toast";

/// The CSS class on a toast's body text.
const TEXT_CLASS: &str = "sk-toast-text";

/// The CSS class on a toast's title header, so a skin can weight it against
/// the body (falls back to the plain text colour unstyled).
const TITLE_CLASS: &str = "sk-toast-title";

/// The CSS class on a toast button — the shared push-button surface.
const BUTTON_CLASS: &str = "sk-button";

/// The checkbox glyph shown when the "don't show me this again" box is ticked
/// (`☑`).
const CHECK_ON: &str = "\u{2611}";

/// The checkbox glyph shown when the box is unticked (`☐`).
const CHECK_OFF: &str = "\u{2610}";

/// The glyph on a toast's close button (`×`).
const CLOSE_GLYPH: &str = "\u{00d7}";

/// The glyph on the overflow control's cycle button (`▸`), which rotates the
/// hidden queue into view.
const CYCLE_GLYPH: &str = "\u{25b8}";

/// The most toasts shown at once; the rest queue (hidden and paused) and are
/// reached by dismissing a visible one or cycling the overflow control — so a
/// flood does not fill the whole edge of the screen (the reference notification
/// well). One at a time: the richer cards (a group-notice card with its image,
/// subject, body and item) are tall enough that even a few would claim the whole
/// screen edge, so the rest wait behind the "N more ▸" control.
const MAX_VISIBLE_TOASTS: usize = 1;

/// The env var that, when set, raises a staggered sequence of sample
/// notifications shortly after startup, so the live stacking / timeout / fade /
/// modal behaviour can be watched without a server alert. A source-level debug
/// affordance (see the memory note on CLI vs env), off by default.
pub const DEMO_ENV: &str = "SL_VIEWER_NOTIFICATION_DEMO";

/// How long the demo waits before raising the corner toasts, in seconds — enough
/// for the async Fluent bundle to load so their text resolves through i18n.
const DEMO_START_DELAY_SECS: f32 = 2.5;

/// How long after the corner toasts the demo pops the modal, in seconds — so the
/// corner stack is seen before the modal's scrim covers it.
const DEMO_MODAL_DELAY_SECS: f32 = 4.0;

/// The kind accent colour painted on a card's border, a subtle cue to the
/// notification's class where the reference viewer uses a per-notification icon
/// texture (which we do not yet carry as data).
const fn kind_accent(kind: NotificationKind) -> Color {
    match kind {
        NotificationKind::Tip => Color::srgb(0.36, 0.62, 0.90),
        NotificationKind::Notify => Color::srgb(0.55, 0.60, 0.68),
        NotificationKind::Alert => Color::srgb(0.95, 0.72, 0.30),
        NotificationKind::AlertModal => Color::srgb(0.90, 0.36, 0.32),
    }
}

/// The plugin: the channel, the raise / age / resolve / dismiss / fade systems,
/// and the ignorable-notification settings. Does **not** wire the live
/// `AlertMessage` source ([`ingest_alert_messages`]) — the viewer binary adds
/// that, so the gallery can host the plugin without the session event stream.
#[derive(Debug)]
pub struct NotificationHostPlugin;

impl Plugin for NotificationHostPlugin {
    /// Register the messages, the manager resource, the channel spawn and the
    /// per-frame systems (ordered so a raise, an expiry and a dismiss all resolve
    /// in the same frame they occur).
    fn build(&self, app: &mut App) {
        app.add_message::<ShowNotification>()
            .add_message::<NotificationResponse>()
            .add_message::<DismissNotification>()
            .add_message::<ResolveNotification>()
            .add_message::<CycleToasts>()
            .init_resource::<NotificationManager>()
            .init_resource::<DoNotDisturbQueue>()
            .add_systems(
                Startup,
                (
                    register_notification_settings,
                    spawn_notification_channel.after(UiScaffoldSystems::SpawnRoot),
                ),
            )
            .add_systems(
                Update,
                (
                    raise_notifications,
                    age_and_fade_toasts,
                    handle_dismiss,
                    resolve_notifications,
                    log_notification_responses,
                    apply_toast_opacity,
                )
                    .chain(),
            )
            // Reorders the stack when a toast lands (keyed on `Added<Toast>`),
            // caps the visible count each frame, and cycles the queue on request —
            // ordered so a fresh sort, then the cap, then a click-cycle compose.
            .add_systems(
                Update,
                (
                    order_channel_by_priority,
                    cycle_toasts,
                    apply_toast_overflow,
                )
                    .chain(),
            );
    }
}

/// Notifications held back while **Do Not Disturb** is on
/// ([`crate::presence`]) — the reference's muted notification channels plus its
/// `LLDoNotDisturbNotificationStorage`: a corner toast raised while the mode is
/// on is not shown, it is *kept*, and every held toast is raised for real the
/// moment the mode is switched off, so nothing is silently lost.
///
/// **Modals are never queued.** A modal is a blocking confirm in front of
/// something the user is doing right now (a quit confirm, a discard-changes
/// prompt); holding one back would deadlock the flow it belongs to. Do Not
/// Disturb suppresses the *unsolicited* interruptions, which is what the corner
/// channel carries.
#[derive(Resource, Debug, Default)]
struct DoNotDisturbQueue {
    /// The raises held back, oldest first.
    held: Vec<ShowNotification>,
    /// Whether Do Not Disturb was on last frame, so the drain runs on the
    /// falling edge only.
    was_busy: bool,
}

/// The channel container and its overflow control, so `raise_notifications`
/// can parent a corner toast to the channel and the overflow systems can drive
/// the "N more" control.
#[derive(Resource, Debug, Clone, Copy)]
pub struct NotificationChannelRoot {
    /// The stacking channel container the toasts are children of. Exposed so a
    /// bespoke-content toast (the group-notice card, `crate::group_notice`) can
    /// join the same stack — and thus the same ordering / overflow-cycling — as
    /// the catalogue toasts, via [`adopt_toast`].
    pub(crate) channel: Entity,
    /// The overflow control (a "N more ▸" cycle button), the last channel child,
    /// hidden until the stack exceeds `MAX_VISIBLE_TOASTS`.
    overflow: Entity,
}

/// Marks the overflow control node ([`NotificationChannelRoot::overflow`]).
#[derive(Component, Debug)]
struct OverflowControl;

/// A live toast's per-frame state, on the entity torn down when it resolves — the
/// corner card, or the modal scrim (whose child is the dialog).
#[derive(Component, Debug)]
struct Toast {
    /// The raised notification's id, echoed on its [`NotificationResponse`].
    id: crate::notifications::NotificationId,
    /// The catalogue template name.
    template: &'static str,
    /// The priority, driving the stack order ([`order_channel_by_priority`]).
    priority: NotificationPriority,
    /// The button chosen on auto-expiry (the reference `expire_option`), or
    /// `None` for a form with no default.
    default_button: Option<&'static str>,
    /// Seconds this toast has been on screen, advanced by [`Time::delta_secs`].
    age: f32,
    /// Seconds before the fade begins, or `0.0` for a kind that never
    /// auto-expires (alerts / modals wait for a click).
    lifetime: f32,
    /// The current opacity `[0, 1]`, `1.0` until the fade, driving
    /// [`apply_toast_opacity`].
    opacity: f32,
    /// Whether the pointer is over the toast, which pauses its timer (the
    /// reference `stopToastTimer`).
    hovered: bool,
    /// Whether the toast is queued off-screen past `MAX_VISIBLE_TOASTS`
    /// ([`apply_toast_overflow`]) — hidden and its timer paused until a visible
    /// toast is dismissed or the overflow control cycles it into view.
    overflowed: bool,
    /// Whether a resolve has already been written for this toast, so
    /// [`age_and_fade_toasts`] does not write a second on the frame before the
    /// despawn is applied.
    resolved: bool,
    /// The text-input field ([`EditableText`] node) for a template with a
    /// [`NotificationTemplate::input`](crate::notifications::NotificationTemplate::input),
    /// read on resolve into [`NotificationResponse::input`].
    input_field: Option<Entity>,
}

/// Marks a node whose colours fade with its toast: the fade system scales the
/// alpha of the node's `BackgroundColor` / `TextColor` by the toast's opacity,
/// capturing each base colour the first frame the fade begins — so a
/// full-opacity toast stays under the skin's control and only the disappearing
/// tail is Rust-driven.
#[derive(Component, Debug)]
struct FadeColor {
    /// The `Toast`-bearing entity whose opacity drives this node.
    toast: Entity,
    /// The captured base background colour, once the fade has begun.
    base_bg: Option<Color>,
    /// The captured base text colour, once the fade has begun.
    base_text: Option<Color>,
}

/// A toast's "don't show me this again" checkbox state.
#[derive(Component, Debug)]
struct IgnoreCheckbox {
    /// Whether the box is ticked.
    checked: bool,
}

/// Internal: the overflow control was clicked — rotate the queued toasts so the
/// next hidden one comes into view (the reference "cycle through the open ones").
#[derive(Message, Debug, Clone, Copy)]
struct CycleToasts;

/// A resolved teardown for one toast, from a button click, an auto-expiry or a
/// [`DismissNotification`]. Centralises teardown so one system despawns the toast,
/// clears the dedup index, persists the ignore flag and emits the public
/// [`NotificationResponse`]. Exposed so a bespoke-content toast (the group-notice
/// card, `crate::group_notice`) tears down through the same path as the
/// catalogue toasts — the reference "close counts as acknowledged".
#[derive(Message, Debug, Clone, Copy)]
pub struct ResolveNotification {
    /// The `Toast`-bearing entity to tear down.
    pub toast: Entity,
    /// The chosen button, or `None` for an expiry / external dismiss.
    pub button: Option<&'static str>,
}

/// Startup: declare each suppressible notification's "show again" flag
/// (default on), so a stored suppression coerces against it and the
/// Preferences alerts tab ([[viewer-preferences-alerts-tab]]) has a
/// registered setting to bind. The storage follows the template's
/// [`NotificationIgnore`] kind: a session-only suppression registers as a
/// transient (never-persisted) setting so it resets every run, and a
/// [`NotificationIgnore::LastResponse`] template additionally gets the
/// [`last_response_setting_name`] `String` holding the button to replay
/// (the reference's `"Default" + name` ignores entry). A
/// [`NotificationIgnore::CheckboxOnly`] template registers nothing — its
/// checkbox state rides the [`NotificationResponse`] for the owner alone.
fn register_notification_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    for entry in NOTIFICATIONS {
        match entry.ignore {
            NotificationIgnore::None | NotificationIgnore::CheckboxOnly => {}
            NotificationIgnore::DefaultResponse
            | NotificationIgnore::ShowAgain
            | NotificationIgnore::LastResponse => {
                settings.register_in(
                    &[NOTIFICATIONS_SECTION],
                    entry.name,
                    SettingValue::Bool(true),
                    "Show this notification (untick to suppress it)",
                );
                if entry.ignore == NotificationIgnore::LastResponse {
                    settings.register_in(
                        &[NOTIFICATIONS_SECTION],
                        &last_response_setting_name(entry.name),
                        SettingValue::String(String::new()),
                        "The button name replayed when this suppressed \
                         notification is raised (empty = the form's default)",
                    );
                }
            }
            NotificationIgnore::DefaultResponseSessionOnly => {
                settings.register_transient(
                    entry.name,
                    SettingValue::Bool(true),
                    "Show this notification (untick to suppress it for this \
                     session)",
                );
            }
        }
    }
}

/// Startup: spawn the toast channel under the UI root and publish its entity.
///
/// An **absolute**, content-sized column pinned to the trailing-**top** corner
/// via a [`LogicalInset`] (so it mirrors to the top-left under RTL). New toasts
/// append below the older ones and [`order_channel_by_priority`] then floats the
/// highest-priority / newest to the top. Transparent and non-blocking; the toast
/// cards themselves take the clicks.
fn spawn_notification_channel(mut commands: Commands, root: Res<UiRoot>) {
    let channel = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                ..column(Val::Px(TOAST_GAP))
            },
            LogicalInset(LogicalRect {
                inline_end: Val::Px(CHANNEL_INLINE_INSET),
                block_start: Val::Px(CHANNEL_BLOCK_INSET),
                ..LogicalRect::AUTO
            }),
            GlobalZIndex(TOAST_CHANNEL_Z),
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("notification-channel"),
            ChildOf(root.0),
        ))
        .id();
    // The overflow control: a "N more ▸" cycle button that hugs the trailing edge
    // below the visible stack, hidden until there are queued toasts. Its Text and
    // display are driven by [`apply_toast_overflow`]; a click cycles the queue.
    let overflow = commands
        .spawn((
            Button,
            OverflowControl,
            TabIndex(0),
            Node {
                display: Display::None,
                align_self: AlignSelf::End,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            Text::default(),
            UiFont::Sans.at(TOAST_FONT_SIZE),
            TextColor(TEXT_COLOR),
            BackgroundColor(CARD_BACKGROUND),
            BorderColor::all(BUTTON_BORDER),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Name::new("notification-overflow"),
            ChildOf(channel),
        ))
        .observe(
            move |_activate: On<Activate>, mut cycle: MessageWriter<CycleToasts>| {
                cycle.write(CycleToasts);
            },
        )
        .id();
    commands.insert_resource(NotificationChannelRoot { channel, overflow });
}

/// The already-resolved content of one toast, ready to render: the live path
/// resolves the catalogue template + i18n into this; the gallery specimen builds
/// it from literals, so both render through the one `build_toast_card`.
struct ToastContent {
    /// The behaviour class (drives the accent and whether it fades).
    kind: NotificationKind,
    /// The resolved dialog title (the reference `label`), rendered as a
    /// header line above the body, or `None` for a card with no title.
    title: Option<String>,
    /// The display-ready body text.
    body: String,
    /// The buttons, with resolved labels.
    buttons: Vec<ToastButtonSpec>,
    /// Whether to show the "don't show me this again" checkbox.
    ignorable: bool,
    /// The resolved ignore-checkbox label (used only when
    /// [`ignorable`](Self::ignorable)).
    ignore_label: String,
    /// Whether to show a close (×) button that dismisses the toast early — for a
    /// corner toast (a fading tip / notify has no other affordance), not for a
    /// modal (which is dismissed by choosing one of its buttons).
    closable: bool,
    /// The text size, in logical pixels.
    font_size: f32,
    /// The resolved, `[KEY]`-substituted initial text for the single-line
    /// input field (the reference `<input>`), or `None` for a form without
    /// one.
    input: Option<String>,
}

/// One button's resolved spec for `build_toast_card`.
struct ToastButtonSpec {
    /// The stable button name (response id).
    name: &'static str,
    /// The resolved, display-ready label.
    label: String,
    /// Whether this is the default button.
    is_default: bool,
}

/// The entities `build_toast_card` produced that a caller wires: the card root,
/// the button boxes (paired with their name), and the ignore checkbox.
struct ToastCard {
    /// The card root node (or, for a modal, the panel reparented under the
    /// scrim).
    root: Entity,
    /// Each button box paired with its [`ToastButtonSpec::name`].
    buttons: Vec<(Entity, &'static str)>,
    /// The ignore checkbox: its clickable box and the glyph text node to flip.
    ignore: Option<(Entity, Entity)>,
    /// The close (×) button box, when the toast is [`closable`](ToastContent::closable).
    close: Option<Entity>,
    /// The text-input field ([`EditableText`] node), when the content carries
    /// an [`input`](ToastContent::input).
    input: Option<Entity>,
}

/// Build a toast card's node tree (not yet parented) from resolved [`ToastContent`],
/// returning the entities a caller wires. Shared by the live host and the
/// gallery specimen so the two render identically — the registry rule
/// ([`crate::ui_element`]).
fn build_toast_card(commands: &mut Commands, content: &ToastContent) -> ToastCard {
    let fades = content.kind.fades();
    // Reserve the root so children can reference it (the fade target) before its
    // own bundle is inserted.
    let root = commands.spawn_empty().id();
    commands.entity(root).insert((
        Node {
            max_width: Val::Px(TOAST_MAX_WIDTH),
            padding: UiRect::all(Val::Px(TOAST_PADDING)),
            border: UiRect::all(Val::Px(TOAST_BORDER)),
            row_gap: Val::Px(CARD_ROW_GAP),
            ..column(Val::Px(CARD_ROW_GAP))
        },
        BackgroundColor(CARD_BACKGROUND),
        BorderColor::all(kind_accent(content.kind)),
        ClassList::new_with_classes([CARD_CLASS]),
        Pickable {
            should_block_lower: true,
            is_hoverable: true,
        },
        Name::new("toast"),
    ));
    if fades {
        commands.entity(root).insert(FadeColor {
            toast: root,
            base_bg: None,
            base_text: None,
        });
    }

    // A close (×) button, top-trailing — the early-dismiss affordance every corner
    // toast carries (a fading tip / notify has no other), matching the reference
    // `LLToast` close button. Spawned first, so it sits above the body.
    let close = if content.closable {
        let close_row = commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::End,
                    ..row(Val::ZERO)
                },
                Name::new("toast-close-row"),
                ChildOf(root),
            ))
            .id();
        let close_button = commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                    ..default()
                },
                ClassList::new_with_classes([BUTTON_CLASS]),
                Name::new("toast-close"),
                ChildOf(close_row),
            ))
            .with_child((
                Text::new(CLOSE_GLYPH),
                UiFont::Sans.at(content.font_size),
                TextColor(TEXT_COLOR),
            ))
            .id();
        Some(close_button)
    } else {
        None
    };

    // The title header, when the content carries one (the reference `label`):
    // its own width-bounded box above the body, same measure-safe shape.
    if let Some(title) = &content.title {
        let title_box = commands
            .spawn((
                Node {
                    max_width: Val::Px(TEXT_MAX_WIDTH),
                    ..default()
                },
                Name::new("toast-title"),
                ChildOf(root),
            ))
            .id();
        let title_text = commands
            .spawn((
                Text::new(title.clone()),
                UiFont::Sans.at(content.font_size),
                TextColor(TEXT_COLOR),
                ClassList::new_with_classes([TITLE_CLASS]),
                Name::new("toast-title-text"),
                ChildOf(title_box),
            ))
            .id();
        if fades {
            commands.entity(title_text).insert(FadeColor {
                toast: root,
                base_bg: None,
                base_text: None,
            });
        }
    }

    // The body: a decoration-free, width-bounded box holding the paragraph as its
    // sole child (the measure-bug constraint).
    let body_box = commands
        .spawn((
            Node {
                max_width: Val::Px(TEXT_MAX_WIDTH),
                ..default()
            },
            Name::new("toast-body"),
            ChildOf(root),
        ))
        .id();
    let text = commands
        .spawn((
            Text::new(content.body.clone()),
            UiFont::Sans.at(content.font_size),
            TextColor(TEXT_COLOR),
            ClassList::new_with_classes([TEXT_CLASS]),
            Name::new("toast-text"),
            ChildOf(body_box),
        ))
        .id();
    if fades {
        commands.entity(text).insert(FadeColor {
            toast: root,
            base_bg: None,
            base_text: None,
        });
    }

    // The text-input row, if the form carries one (the reference `<input>`):
    // a single-line field between the body and the buttons, filling the card
    // width so the container — not the pre-filled text — decides its size.
    let input = content.input.as_ref().map(|initial| {
        let input_row = commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    ..row(Val::ZERO)
                },
                Name::new("toast-input-row"),
                ChildOf(root),
            ))
            .id();
        let spec = TextInputSpec {
            initial: initial.clone(),
            tab_index: 1,
            font_size: content.font_size,
            fill: true,
            ..TextInputSpec::new("toast-input", TextInputKind::Line)
        };
        spawn_text_input(commands, input_row, &spec)
    });

    // The button row, if any.
    let mut buttons = Vec::new();
    if !content.buttons.is_empty() {
        let button_row = commands
            .spawn((
                Node {
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: Val::Px(BUTTON_GAP),
                    justify_content: JustifyContent::End,
                    ..row(Val::Px(BUTTON_GAP))
                },
                Name::new("toast-buttons"),
                ChildOf(root),
            ))
            .id();
        for spec in &content.buttons {
            // The default button (Enter / expiry) wears the kind accent on its
            // border, the reference's emphasis on the default action.
            let border = if spec.is_default {
                kind_accent(content.kind)
            } else {
                BUTTON_BORDER
            };
            let button = commands
                .spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_BACKGROUND),
                    BorderColor::all(border),
                    ClassList::new_with_classes([BUTTON_CLASS]),
                    Name::new(format!("toast-button:{}", spec.name)),
                    ChildOf(button_row),
                ))
                .with_child((
                    Text::new(spec.label.clone()),
                    UiFont::Sans.at(content.font_size),
                    TextColor(TEXT_COLOR),
                ))
                .id();
            buttons.push((button, spec.name));
        }
    }

    // The ignore checkbox row, if the notification is ignorable.
    let ignore = if content.ignorable {
        let ignore_row = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    // Wrap the label under the box when a long / large-font
                    // translation outgrows the row.
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: Val::Px(4.0),
                    ..row(Val::Px(6.0))
                },
                Name::new("toast-ignore"),
                ChildOf(root),
            ))
            .id();
        let glyph = commands
            .spawn((
                Text::new(CHECK_OFF),
                UiFont::Sans.at(content.font_size),
                TextColor(TEXT_COLOR),
                Name::new("toast-ignore-glyph"),
            ))
            .id();
        let checkbox = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    padding: UiRect::all(Val::Px(2.0)),
                    ..row(Val::Px(6.0))
                },
                IgnoreCheckbox { checked: false },
                Name::new("toast-ignore-box"),
                ChildOf(ignore_row),
            ))
            .add_child(glyph)
            .id();
        // The label is bounded (and the sole occupant of its box) so a long /
        // large-font translation wraps inside the card instead of overflowing it —
        // the same measure-safe pattern as the body (see `build_toast_card`).
        let label_box = commands
            .spawn((
                Node {
                    max_width: Val::Px(TEXT_MAX_WIDTH - IGNORE_CHECKBOX_ALLOWANCE),
                    ..default()
                },
                Name::new("toast-ignore-label-box"),
                ChildOf(ignore_row),
            ))
            .id();
        commands.spawn((
            Text::new(content.ignore_label.clone()),
            UiFont::Sans.at(content.font_size),
            TextColor(TEXT_COLOR),
            ClassList::new_with_classes([TEXT_CLASS]),
            Name::new("toast-ignore-label"),
            ChildOf(label_box),
        ));
        Some((checkbox, glyph))
    } else {
        None
    };

    ToastCard {
        root,
        buttons,
        ignore,
        close,
        input,
    }
}

/// Adopt an externally-built card `root` as a managed toast in the shared corner
/// channel — the way a bespoke-content notification (the group-notice card,
/// `crate::group_notice`) joins the catalogue toasts so it inherits the same
/// priority ordering and "N more" overflow-cycling rather than owning a second
/// channel.
///
/// Parents the card into the channel, tags it with a `Toast` (of the given
/// `kind` — its [`lifetime_secs`](NotificationKind::lifetime_secs) drives whether
/// it fades or sticks), and records a history entry. The caller is responsible for
/// wiring the card's own dismiss affordances to a [`ResolveNotification`] (so a
/// **user** close is what ends it — display alone never does). Returns the toast's
/// [`NotificationId`](crate::notifications::NotificationId).
#[expect(
    clippy::too_many_arguments,
    reason = "the toast's identity is genuinely this many independent facts (channel, kind, \
              priority, template, default button, history body) plus the commands / manager / \
              root it acts on; bundling them into a struct would only move the list, not shorten it"
)]
pub fn adopt_toast(
    commands: &mut Commands,
    manager: &mut NotificationManager,
    channel: &NotificationChannelRoot,
    root: Entity,
    kind: NotificationKind,
    priority: NotificationPriority,
    template: &'static str,
    default_button: Option<&'static str>,
    history_body: String,
) -> crate::notifications::NotificationId {
    let id = manager.allocate_id();
    commands.entity(root).insert((
        Toast {
            id,
            template,
            priority,
            default_button,
            age: 0.0,
            lifetime: kind.lifetime_secs(),
            opacity: 1.0,
            hovered: false,
            overflowed: false,
            resolved: false,
            input_field: None,
        },
        ChildOf(channel.channel),
    ));
    manager.push_history(NotificationRecord {
        id,
        template,
        kind,
        body: history_body,
        response: None,
    });
    id
}

/// Spawn a full-screen modal scrim under the UI root, centring its child dialog
/// and blocking the world behind it.
fn spawn_modal_scrim(commands: &mut Commands, root: Entity) -> Entity {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            LogicalInset(LogicalRect {
                inline_start: Val::Px(0.0),
                block_start: Val::Px(0.0),
                ..LogicalRect::AUTO
            }),
            BackgroundColor(SCRIM_COLOR),
            GlobalZIndex(MODAL_SCRIM_Z),
            // Blocks every click behind it — the whole point of a modal.
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("modal-scrim"),
            ChildOf(root),
        ))
        .id()
}

/// Whether the named notification is currently suppressed (`show again` unticked)
/// in the settings store.
fn is_suppressed(settings: Option<&ViewerSettings>, name: &str) -> bool {
    settings
        .and_then(|settings| settings.store().get_bool(name).ok())
        .is_some_and(|show| !show)
}

/// The button a suppressed raise auto-responds with — the reference
/// `handleIgnoredNotification` per [`NotificationIgnore`] kind: the form's
/// default button for the default-response kinds, the saved
/// [`last_response_setting_name`] button (falling back to the default when
/// none is saved or the saved name no longer matches a form button) for
/// [`NotificationIgnore::LastResponse`], and nothing for
/// [`NotificationIgnore::ShowAgain`]. `None` / `CheckboxOnly` templates are
/// never suppressed, so the answer for them is moot (`None`).
fn auto_response_button(
    tmpl: &NotificationTemplate,
    settings: Option<&ViewerSettings>,
) -> Option<&'static str> {
    match tmpl.ignore {
        NotificationIgnore::DefaultResponse | NotificationIgnore::DefaultResponseSessionOnly => {
            tmpl.default_button()
        }
        NotificationIgnore::LastResponse => settings
            .and_then(|settings| {
                let setting = last_response_setting_name(tmpl.name);
                let saved = settings.store().get_str(&setting).ok()?;
                tmpl.form
                    .iter()
                    .find(|button| button.name == saved)
                    .map(|button| button.name)
            })
            .or_else(|| tmpl.default_button()),
        NotificationIgnore::ShowAgain
        | NotificationIgnore::None
        | NotificationIgnore::CheckboxOnly => None,
    }
}

/// The label on the toast's ignore checkbox, by [`NotificationIgnore`] kind
/// (the reference `LLToastPanel` variants): the plain "don't show me this
/// again" for the default kinds, its session-only variant, "always choose
/// this option" for [`NotificationIgnore::LastResponse`], and — for
/// [`NotificationIgnore::CheckboxOnly`] — the template's own
/// [`ignore_key`](NotificationTemplate::ignore_key) text, which in the
/// reference *is* the checkbox label ("Remember this computer for 30
/// days."). Templates with no checkbox resolve to the plain label, unused.
fn ignore_checkbox_label(tmpl: &NotificationTemplate, translator: &Translator) -> String {
    match tmpl.ignore {
        NotificationIgnore::CheckboxOnly => tmpl.ignore_key.map_or_else(
            || translator.get("notification-ignore-checkbox"),
            |key| translator.get(key),
        ),
        NotificationIgnore::DefaultResponseSessionOnly => {
            translator.get("notification-ignore-checkbox-session")
        }
        NotificationIgnore::LastResponse => translator.get("notification-ignore-choice"),
        NotificationIgnore::None
        | NotificationIgnore::DefaultResponse
        | NotificationIgnore::ShowAgain => translator.get("notification-ignore-checkbox"),
    }
}

/// Raise the queued [`ShowNotification`]s: look up the catalogue template, honour
/// suppression and `unique` dedup, resolve the body + button labels through
/// i18n, build the toast (corner card or modal scrim) and wire its buttons and
/// ignore checkbox.
#[expect(
    clippy::too_many_arguments,
    reason = "a raise needs the request queue, the manager, the channel/root, i18n, \
              suppression settings, the dismiss channel, the persistence channel and the \
              response channel (a suppressed raise auto-responds); each is distinct"
)]
fn raise_notifications(
    mut commands: Commands,
    mut requests: MessageReader<ShowNotification>,
    mut manager: ResMut<NotificationManager>,
    channel: Option<Res<NotificationChannelRoot>>,
    root: Res<UiRoot>,
    translator: Translator,
    settings: Option<Res<ViewerSettings>>,
    presence: Option<Res<crate::world_api::PresenceState>>,
    mut queue: ResMut<DoNotDisturbQueue>,
    mut dismiss: MessageWriter<DismissNotification>,
    mut chat: MessageWriter<LocalChatNotice>,
    mut persist: MessageWriter<PersistNotification>,
    mut responses: MessageWriter<NotificationResponse>,
) {
    let Some(channel) = channel else {
        return;
    };
    // Do Not Disturb holds the corner channel back and replays it on the way
    // out — so the raises this frame are the fresh ones plus, on the falling
    // edge, everything that was held.
    let busy = presence.is_some_and(|presence| presence.is_do_not_disturb());
    let mut pending: Vec<ShowNotification> = if !busy && queue.was_busy {
        let held = std::mem::take(&mut queue.held);
        if !held.is_empty() {
            info!(
                "notification: do-not-disturb ended, showing {} held notification(s)",
                held.len()
            );
        }
        held
    } else {
        Vec::new()
    };
    queue.was_busy = busy;
    pending.extend(requests.read().cloned());
    for request in &pending {
        let Some(tmpl) = template(request.template) else {
            warn!(
                "notification: dropping raise of unknown template {}",
                request.template
            );
            continue;
        };
        // Do Not Disturb: hold the corner toast for later rather than
        // interrupting. A modal is never held (see [`DoNotDisturbQueue`]).
        if busy && !tmpl.kind.is_modal() {
            queue.held.push(request.clone());
            debug!(template = tmpl.name, "notification held for do-not-disturb");
            continue;
        }
        // A suppressed raise is not shown, but it still *answers* — the
        // reference `handleIgnoredNotification`: the default button (or the
        // saved last response) fires so the confirmed action proceeds
        // instead of silently doing nothing. `ShowAgain` alone stays mute.
        if tmpl.ignore.is_suppressible() && is_suppressed(settings.as_deref(), tmpl.name) {
            debug!(template = tmpl.name, "notification suppressed");
            responses.write(NotificationResponse {
                id: manager.allocate_id(),
                template: tmpl.name,
                button: auto_response_button(tmpl, settings.as_deref()),
                ignored: true,
                input: None,
            });
            continue;
        }
        // `unique`: a repeat with the same context replaces its predecessor.
        if tmpl.unique
            && let Some(previous) = manager.live_unique(tmpl.name, request.context.as_deref())
        {
            dismiss.write(DismissNotification { id: previous });
        }

        let raw = request
            .body
            .clone()
            .unwrap_or_else(|| translator.get(tmpl.message_key));
        let body = substitute(&raw, &request.args);
        let id = manager.allocate_id();
        // Button labels are `[KEY]`-substituted like the body, so a dynamic
        // reference label ("Create group for L$[COST]", "[ACTION] Now")
        // resolves from the raise's args.
        let buttons = tmpl
            .form
            .iter()
            .map(|button| ToastButtonSpec {
                name: button.name,
                label: substitute(&translator.get(button.label_key), &request.args),
                is_default: button.is_default,
            })
            .collect();
        // The input field's pre-filled text resolves like the body: through
        // i18n, then `[KEY]`-substituted with the raise's args (the reference
        // defaults are templates like `[DESC] (new)`). A field with no
        // default key starts empty (the announcement prompts).
        let input = tmpl.input.map(|input| {
            input
                .default_key
                .map(|key| substitute(&translator.get(key), &request.args))
                .unwrap_or_default()
        });
        let content = ToastContent {
            kind: tmpl.kind,
            // The title substitutes too — the reference offer labels carry
            // [NAME_LABEL].
            title: tmpl
                .title_key
                .map(|key| substitute(&translator.get(key), &request.args)),
            body: body.clone(),
            buttons,
            ignorable: tmpl.ignore.offers_checkbox(),
            ignore_label: ignore_checkbox_label(tmpl, &translator),
            // A modal is dismissed by choosing a button; every corner toast gets a
            // close × so a fading tip / notify can be dismissed early.
            closable: !tmpl.kind.is_modal(),
            font_size: TOAST_FONT_SIZE,
            input,
        };
        let card = build_toast_card(&mut commands, &content);

        // A modal lives on a centred scrim; every other kind is a corner toast.
        let toast_entity = if tmpl.kind.is_modal() {
            let scrim = spawn_modal_scrim(&mut commands, root.0);
            commands.entity(card.root).insert(ChildOf(scrim));
            scrim
        } else {
            commands.entity(card.root).insert(ChildOf(channel.channel));
            card.root
        };
        commands.entity(toast_entity).insert(Toast {
            id,
            template: tmpl.name,
            priority: tmpl.priority,
            default_button: tmpl.default_button(),
            age: 0.0,
            lifetime: tmpl.kind.lifetime_secs(),
            opacity: 1.0,
            hovered: false,
            overflowed: false,
            resolved: false,
            input_field: card.input,
        });

        // Hovering pauses a fading toast's timer.
        if tmpl.kind.fades() {
            let target = toast_entity;
            commands
                .entity(card.root)
                .observe(
                    move |_over: On<Pointer<Over>>, mut toasts: Query<&mut Toast>| {
                        if let Ok(mut toast) = toasts.get_mut(target) {
                            toast.hovered = true;
                        }
                    },
                )
                .observe(
                    move |_out: On<Pointer<Out>>, mut toasts: Query<&mut Toast>| {
                        if let Ok(mut toast) = toasts.get_mut(target) {
                            toast.hovered = false;
                        }
                    },
                );
        }

        // Wire each button to a resolve carrying its name. With an input field
        // the field holds tab stop 1, so the buttons start after it.
        let button_tab_base = if card.input.is_some() { 2 } else { 1 };
        for (index, (button, name)) in card.buttons.iter().enumerate() {
            let target = toast_entity;
            let name = *name;
            let tab = i32::try_from(index)
                .unwrap_or(0)
                .saturating_add(button_tab_base);
            commands
                .entity(*button)
                .insert((Button, TabIndex(tab)))
                .observe(
                    move |_activate: On<Activate>,
                          mut resolves: MessageWriter<ResolveNotification>| {
                        resolves.write(ResolveNotification {
                            toast: target,
                            button: Some(name),
                        });
                    },
                );
        }

        // Wire the ignore checkbox to toggle its state and glyph.
        if let Some((checkbox, glyph)) = card.ignore {
            commands
                .entity(checkbox)
                .insert((Button, TabIndex(0)))
                .observe(
                    move |_activate: On<Activate>,
                          mut boxes: Query<&mut IgnoreCheckbox>,
                          mut texts: Query<&mut Text>| {
                        if let Ok(mut state) = boxes.get_mut(checkbox) {
                            state.checked = !state.checked;
                            if let Ok(mut text) = texts.get_mut(glyph) {
                                let glyph_text = if state.checked { CHECK_ON } else { CHECK_OFF };
                                text.0.clear();
                                text.0.push_str(glyph_text);
                            }
                        }
                    },
                );
        }

        // Wire the close (×) button to dismiss the toast early (no button choice).
        if let Some(close) = card.close {
            let target = toast_entity;
            commands
                .entity(close)
                .insert((Button, TabIndex(0)))
                .observe(
                    move |_activate: On<Activate>,
                          mut resolves: MessageWriter<ResolveNotification>| {
                        resolves.write(ResolveNotification {
                            toast: target,
                            button: None,
                        });
                    },
                );
        }

        // Honour `log_to_chat`: echo the body into the nearby-chat overlay, as the
        // reference does for a notification tagged `log_to_chat`.
        if tmpl.log_to_chat {
            chat.write(LocalChatNotice::new(body.clone()));
        }
        manager.push_history(NotificationRecord {
            id,
            template: tmpl.name,
            kind: tmpl.kind,
            body,
            response: None,
        });
        if tmpl.unique {
            manager.register_unique(tmpl.name, request.context.as_deref(), id);
        }
        // Persist a sticky (non-fading) `persist` notification so it re-displays
        // after a relog until the user answers it — the reference
        // `LLPersistentNotificationStorage`. A fading tip / notify is transient by
        // nature and is not persisted.
        if tmpl.persist && !tmpl.kind.fades() {
            persist.write(PersistNotification {
                id,
                kind: PersistedKind::Catalogue {
                    template: tmpl.name.to_owned(),
                    args: request.args.pairs().to_vec(),
                    body: request.body.clone(),
                    context: request.context.clone(),
                },
            });
        }
        debug!(
            template = tmpl.name,
            persist = tmpl.persist,
            "notification raised"
        );
    }
}

/// Advance each fading toast's age (paused while hovered), fade it over the last
/// [`crate::notifications::TOAST_FADE_SECS`], and resolve it (with its default
/// button) once it has fully faded. Alerts and modals ([`lifetime`](Toast::lifetime)
/// `0`) never auto-expire.
fn age_and_fade_toasts(
    time: Res<Time>,
    mut toasts: Query<(Entity, &mut Toast)>,
    mut resolves: MessageWriter<ResolveNotification>,
) {
    let dt = time.delta_secs();
    for (entity, mut toast) in &mut toasts {
        if toast.lifetime <= 0.0 || toast.resolved {
            continue;
        }
        // Paused while hovered (the reference `stopToastTimer`) or queued
        // off-screen past the visible cap, so a queued toast does not expire unseen.
        if toast.hovered || toast.overflowed {
            continue;
        }
        toast.age += dt;
        let total = toast.lifetime + crate::notifications::TOAST_FADE_SECS;
        if toast.age >= total {
            toast.resolved = true;
            resolves.write(ResolveNotification {
                toast: entity,
                button: toast.default_button,
            });
        } else if toast.age > toast.lifetime {
            let faded = (toast.age - toast.lifetime) / crate::notifications::TOAST_FADE_SECS;
            toast.opacity = (1.0 - faded).clamp(0.0, 1.0);
        } else {
            toast.opacity = 1.0;
        }
    }
}

/// Turn each [`DismissNotification`] into a [`ResolveNotification`] (no button)
/// for the toast bearing that id, if it is still live.
fn handle_dismiss(
    mut dismissals: MessageReader<DismissNotification>,
    toasts: Query<(Entity, &Toast)>,
    mut resolves: MessageWriter<ResolveNotification>,
) {
    for dismissal in dismissals.read() {
        if let Some((entity, _toast)) = toasts
            .iter()
            .find(|(_entity, toast)| toast.id == dismissal.id)
        {
            resolves.write(ResolveNotification {
                toast: entity,
                button: None,
            });
        }
    }
}

/// Resolve each teardown: record the response and (if ticked) the suppression,
/// clear the dedup index, emit the public [`NotificationResponse`], and despawn
/// the toast (recursively, so a modal's scrim and dialog both go). Deduplicates
/// repeat resolves for one toast within a frame.
#[expect(
    clippy::too_many_arguments,
    reason = "a resolve needs the resolve queue, the manager, the toast + descendant \
              queries for the ignore state and the input field's text, the response channel \
              and the settings; each is distinct"
)]
fn resolve_notifications(
    mut commands: Commands,
    mut resolutions: MessageReader<ResolveNotification>,
    mut manager: ResMut<NotificationManager>,
    toasts: Query<&Toast>,
    children: Query<&Children>,
    checkboxes: Query<&IgnoreCheckbox>,
    editors: Query<&EditableText>,
    mut responses: MessageWriter<NotificationResponse>,
    mut settings: Option<ResMut<ViewerSettings>>,
) {
    let mut handled: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for resolution in resolutions.read() {
        if !handled.insert(resolution.toast) {
            continue;
        }
        let Ok(toast) = toasts.get(resolution.toast) else {
            // Already torn down (e.g. a stale expiry after a click).
            continue;
        };
        let ignored = children
            .iter_descendants(resolution.toast)
            .any(|node| checkboxes.get(node).is_ok_and(|checkbox| checkbox.checked));
        // A ticked checkbox records what its kind means: a suppression for
        // the suppressible kinds (plus, for `LastResponse`, the button to
        // replay — the reference saves the response under `Default<name>`),
        // and nothing at all for `CheckboxOnly`, whose state rides the
        // response for the template's owner alone.
        if ignored
            && let Some(settings) = settings.as_deref_mut()
            && let Some(tmpl) = template(toast.template)
            && tmpl.ignore.is_suppressible()
        {
            settings.set_account(toast.template, SettingValue::Bool(false));
            if tmpl.ignore == NotificationIgnore::LastResponse
                && let Some(button) = resolution.button
            {
                settings.set_account(
                    &last_response_setting_name(toast.template),
                    SettingValue::String(button.to_owned()),
                );
            }
        }
        manager.record_response(toast.id, resolution.button);
        manager.clear_unique(toast.id);
        // The input field's edited text, for a template that carries one —
        // read only when a button was chosen (a dismissal submits nothing).
        let input = resolution
            .button
            .and(toast.input_field)
            .and_then(|field| editors.get(field).ok())
            .map(|editor| editor.value().to_string());
        responses.write(NotificationResponse {
            id: toast.id,
            template: toast.template,
            button: resolution.button,
            ignored,
            input,
        });
        commands.entity(resolution.toast).despawn();
    }
}

/// Scale each fading node's colours by its toast's opacity, capturing the base
/// colour the first frame the fade begins (see [`FadeColor`]). A full-opacity
/// toast is left under the skin's control.
fn apply_toast_opacity(
    toasts: Query<&Toast>,
    mut faders: Query<(
        &mut FadeColor,
        Option<&mut BackgroundColor>,
        Option<&mut TextColor>,
    )>,
) {
    for (mut fade, background, text) in &mut faders {
        let opacity = toasts.get(fade.toast).map_or(1.0, |toast| toast.opacity);
        if opacity >= 1.0 {
            // Under the skin's control at full opacity; reset so a re-fade
            // recaptures a possibly re-themed colour.
            fade.base_bg = None;
            fade.base_text = None;
            continue;
        }
        if let Some(mut background) = background {
            let base = *fade.base_bg.get_or_insert(background.0);
            background.0 = base.with_alpha(base.alpha() * opacity);
        }
        if let Some(mut text) = text {
            let base = *fade.base_text.get_or_insert(text.0);
            text.0 = base.with_alpha(base.alpha() * opacity);
        }
    }
}

/// Keep the corner channel ordered by priority: a higher-priority toast floats to
/// the more visible top of the stack, ties broken by age so a newer toast of
/// equal priority sits above an older one. Runs when a toast is added (a removal
/// preserves the surviving order, so no reorder is needed).
fn order_channel_by_priority(
    mut commands: Commands,
    channel: Option<Res<NotificationChannelRoot>>,
    added: Query<(), Added<Toast>>,
    children: Query<&Children>,
    toasts: Query<&Toast>,
) {
    if added.is_empty() {
        return;
    }
    let Some(channel) = channel else {
        return;
    };
    let Ok(current) = children.get(channel.channel) else {
        return;
    };
    let mut ordered: Vec<Entity> = current
        .iter()
        .filter(|entity| toasts.contains(*entity))
        .collect();
    ordered.sort_by(
        |first, second| match (toasts.get(*first), toasts.get(*second)) {
            // Highest priority first (top); within a priority, the newer (smaller
            // age) first, so a fresh toast of equal priority sits above an older.
            (Ok(first), Ok(second)) => second
                .priority
                .cmp(&first.priority)
                .then_with(|| first.age.total_cmp(&second.age)),
            _unexpected => Ordering::Equal,
        },
    );
    // The overflow control stays the last child, below the stack.
    ordered.push(channel.overflow);
    commands.entity(channel.channel).add_children(&ordered);
}

/// Cap the visible stack at `MAX_VISIBLE_TOASTS`: show the top toasts in the
/// channel's order and hide (and pause) the rest, then drive the overflow control
/// — shown as a "N more ▸" button when toasts are queued, hidden otherwise. Runs
/// every frame (cheap: a handful of nodes), so a dismissal promotes the next
/// queued toast and the count stays current.
fn apply_toast_overflow(
    channel: Option<Res<NotificationChannelRoot>>,
    translator: Translator,
    children: Query<&Children>,
    mut toasts: Query<(&mut Toast, &mut Node)>,
    mut control_node: Query<&mut Node, (With<OverflowControl>, Without<Toast>)>,
    mut control_text: Query<&mut Text, With<OverflowControl>>,
) {
    let Some(channel) = channel else {
        return;
    };
    let Ok(current) = children.get(channel.channel) else {
        return;
    };
    let mut shown: usize = 0;
    for child in current.iter() {
        let Ok((mut toast, mut node)) = toasts.get_mut(child) else {
            continue;
        };
        let visible = shown < MAX_VISIBLE_TOASTS;
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if toast.overflowed == visible {
            toast.overflowed = !visible;
        }
        shown = shown.saturating_add(1);
    }
    let hidden = shown.saturating_sub(MAX_VISIBLE_TOASTS);
    if let Ok(mut node) = control_node.get_mut(channel.overflow) {
        let display = if hidden > 0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    if hidden > 0
        && let Ok(mut text) = control_text.get_mut(channel.overflow)
    {
        let count = i64::try_from(hidden).unwrap_or(i64::MAX);
        let label = format!(
            "{} {CYCLE_GLYPH}",
            translator.format(
                "notification-overflow",
                &crate::i18n::TransArgs::new().int("count", count),
            )
        );
        if text.0 != label {
            text.0 = label;
        }
    }
}

/// Rotate the queued toasts on a [`CycleToasts`] (the overflow control was
/// clicked): move the top toast to the back so the next hidden one comes into
/// view. A new toast re-sorts the stack ([`order_channel_by_priority`]); this is
/// the manual page-through in between.
fn cycle_toasts(
    mut commands: Commands,
    mut events: MessageReader<CycleToasts>,
    channel: Option<Res<NotificationChannelRoot>>,
    children: Query<&Children>,
    toasts: Query<&Toast>,
) {
    if events.read().count() == 0 {
        return;
    }
    let Some(channel) = channel else {
        return;
    };
    let Ok(current) = children.get(channel.channel) else {
        return;
    };
    let mut ordered: Vec<Entity> = current
        .iter()
        .filter(|entity| toasts.contains(*entity))
        .collect();
    if ordered.len() <= MAX_VISIBLE_TOASTS {
        return;
    }
    ordered.rotate_left(1);
    ordered.push(channel.overflow);
    commands.entity(channel.channel).add_children(&ordered);
}

/// Log each resolved notification's outcome against its history record — the
/// host's own consumer of the [`NotificationResponse`] stream until the specific
/// dialog tasks wire their protocol replies, and a useful diagnostic (every
/// notification's content and answer, one line). Reads the history ring the
/// future list / history panel ([[viewer-notification-history]]) renders.
fn log_notification_responses(
    mut responses: MessageReader<NotificationResponse>,
    manager: Res<NotificationManager>,
) {
    for response in responses.read() {
        if let Some(record) = manager.history().find(|record| record.id == response.id) {
            debug!(
                template = record.template,
                kind = ?record.kind,
                body = record.body.as_str(),
                recorded = ?record.response,
                button = ?response.button,
                ignored = response.ignored,
                input = ?response.input,
                "notification resolved"
            );
        } else {
            debug!(
                template = response.template,
                button = ?response.button,
                ignored = response.ignored,
                input = ?response.input,
                "notification resolved (not in history)"
            );
        }
    }
}

/// **The live source** (viewer-only): surface the simulator's `AlertMessage` and
/// `AgentAlertMessage` — a stream nothing consumed before — as notifications. A
/// modal `AgentAlertMessage` becomes the modal [`GenericAlert`] confirm; a plain
/// alert becomes a [`SystemMessage`] notify. A keyed `AlertInfo` whose key names
/// a catalogue template raises that template with the `ExtraParams` as `[KEY]`
/// substitutions.
///
/// [`GenericAlert`]: crate::notifications::NOTIFICATIONS
/// [`SystemMessage`]: crate::notifications::NOTIFICATIONS
pub fn ingest_alert_messages(
    mut events: MessageReader<SlEvent>,
    mut show: MessageWriter<ShowNotification>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::AlertMessage {
                message,
                alert_info,
                ..
            } => {
                // A keyed alert whose key is in the catalogue raises that template
                // with the parameters substituted; otherwise the plain string is a
                // generic system message.
                let keyed = alert_info
                    .iter()
                    .find_map(|info| template(&info.message).map(|tmpl| (info, tmpl)));
                if let Some((info, tmpl)) = keyed {
                    let mut request = ShowNotification::new(tmpl.name);
                    request.args = crate::notifications::NotificationArgs::parse_extra_params(
                        &info.extra_params,
                    );
                    show.write(request);
                    continue;
                }
                if !message.is_empty() {
                    show.write(ShowNotification::new("SystemMessage").with_body(message.clone()));
                }
            }
            SlSessionEvent::AgentAlertMessage { modal, message, .. } => {
                if message.is_empty() {
                    continue;
                }
                let template_name = if *modal {
                    "GenericAlert"
                } else {
                    "SystemMessage"
                };
                show.write(ShowNotification::new(template_name).with_body(message.clone()));
            }
            _other => {}
        }
    }
}

/// Whether `key`'s last raise is at least [`COMMAND_FAILURE_COOLDOWN`] old,
/// recording `now` as its latest raise when it is.
///
/// The one coalescing rule the two protocol-report systems share: both watch a
/// stream that can repeat every frame while the session is unhealthy, and both
/// want at most one toast per distinct subject per window. Returning `false`
/// suppresses only the *toast* — the failure itself is already logged where it
/// happened.
fn off_cooldown<K: Eq + std::hash::Hash>(
    last_raised: &mut HashMap<K, Duration>,
    key: K,
    now: Duration,
) -> bool {
    if last_raised
        .get(&key)
        .is_some_and(|raised| now.saturating_sub(*raised) < COMMAND_FAILURE_COOLDOWN)
    {
        return false;
    }
    let _previous = last_raised.insert(key, now);
    true
}

/// How long the same command's send-failure notification is suppressed after
/// being raised. The driver queues per-frame commands (camera, controls), so a
/// circuit that is down would otherwise raise the same failure every frame; the
/// network thread still logs each occurrence.
pub const COMMAND_FAILURE_COOLDOWN: Duration = Duration::from_secs(10);

/// **The live source** (viewer-only): surface a queued protocol command whose
/// send **failed** — the request never reached the simulator, so the user's
/// delete, rez, terraform or parcel edit did nothing — as a
/// [`ViewerCommandSendFailed`] toast naming the action and the reason.
///
/// Raises are coalesced per command name on a [`COMMAND_FAILURE_COOLDOWN`], and
/// the catalogue entry is `unique` with the command name as its context, so a
/// command that fails every frame replaces its own toast rather than stacking.
/// Suppressing a *raise* is not suppressing the *error*: the driver's network
/// thread logs every failure as it happens.
///
/// [`ViewerCommandSendFailed`]: crate::notifications::NOTIFICATIONS
pub fn announce_command_failures(
    mut failures: MessageReader<SlCommandFailed>,
    mut show: MessageWriter<ShowNotification>,
    time: Res<Time>,
    mut last_raised: Local<HashMap<&'static str, Duration>>,
) {
    let now = time.elapsed();
    for failure in failures.read() {
        if !off_cooldown(&mut last_raised, failure.command, now) {
            continue;
        }
        show.write(
            ShowNotification::new("ViewerCommandSendFailed")
                .arg("COMMAND", failure.command.to_owned())
                .arg("REASON", failure.error.to_string())
                // The `unique` context: one live toast per failing command.
                .with_context(failure.command.to_owned()),
        );
    }
}

/// The name of the developer setting that turns protocol-diagnostic collection
/// on and off (section `diagnostics`).
pub const SETTING_COLLECT_DIAGNOSTICS: &str = "CollectProtocolDiagnostics";

/// The settings section the diagnostics knobs live under.
const DIAGNOSTICS_SECTION: &[&str] = &["diagnostics"];

/// The `ExpectedReplyMissing` request labels that reach the **user** rather
/// than only the log.
///
/// An allowlist, not a filter, because the label is an open vocabulary and most
/// of what lands in it is not the user's business:
///
/// - a **capability** name, when a driver reports a failed CAPS request this
///   way. Most are background fetches, and a capability the grid does not
///   implement at all fails on every login (stock OpenSim and
///   `SimulatorFeatures`) — a permanent toast about a grid's feature set.
/// - a **message** name, when a reliable packet exhausted its retransmissions.
///   The session tears the circuit down straight after, so the disconnect is
///   the news; a toast naming the packet would arrive alongside it.
/// - [`Diagnostic::LOGOUT_REQUEST`], which the logout itself already surfaces.
///
/// What is left is [`Diagnostic::SIT_REQUEST`]: the session keeps running,
/// nothing else is surfaced, and the agent is simply left standing. New entries
/// belong here only when the same three things are true — the user asked for
/// it, it silently did not happen, and nothing else says so.
pub const USER_VISIBLE_REQUESTS: &[&str] = &[Diagnostic::SIT_REQUEST];

/// How many distinct "expected, unmodelled" diagnostics are remembered for the
/// log-once rule before it degrades to logging every occurrence.
///
/// The keys are grid-controlled strings (a capability event name), so the set is
/// bounded rather than trusted; a real grid produces a few dozen.
const DIAGNOSTIC_LOG_ONCE_CAP: usize = 256;

/// Register the diagnostics developer settings.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        DIAGNOSTICS_SECTION,
        SETTING_COLLECT_DIAGNOSTICS,
        SettingValue::Bool(true),
        "Collect protocol diagnostics (decode failures, unhandled messages, \
         unknown capability events, missing replies) and report them to the log. \
         Costs a little per inbound message; turn it off to run the session lean",
    );
}

/// **The live source** (viewer-only): drain the protocol
/// [`Diagnostic`] stream — the anomalies the session
/// would otherwise silently drop — into the log, and raise a
/// [`ViewerRequestNoReply`] toast for the one class a *user* can feel.
///
/// Level per variant, because the five are not equally interesting:
///
/// - `DecodeFailed` and `CapsDecodeFailed` are genuine protocol gaps in this
///   client and are rare, so they are `warn`; the failed decode's captured bytes
///   follow at `debug`, since a hexdump does not belong in a warning line.
/// - `ExpectedReplyMissing` means something the session asked for was never
///   answered — `warn`, plus the toast below.
/// - `UnhandledMessage` and `UnknownCapsEvent` are *expected*: they name traffic
///   this client does not model, they repeat for every arrival, and on some
///   grids they never stop. They are `debug`, and each distinct one is logged
///   **once** — building the dedup key only when that level is actually on, so
///   the ordinary run pays nothing for them.
///
/// Only `ExpectedReplyMissing` reaches the screen, and only for the labels in
/// [`USER_VISIBLE_REQUESTS`] — coalesced per label like
/// [`announce_command_failures`]. See that list for why it is an allowlist.
///
/// [`ViewerRequestNoReply`]: crate::notifications::NOTIFICATIONS
pub fn ingest_protocol_diagnostics(
    mut diagnostics: MessageReader<SlDiagnostic>,
    mut show: MessageWriter<ShowNotification>,
    time: Res<Time>,
    mut logged_once: Local<HashSet<String>>,
    mut last_raised: Local<HashMap<String, Duration>>,
) {
    let now = time.elapsed();
    for SlDiagnostic(diagnostic) in diagnostics.read() {
        match diagnostic {
            Diagnostic::UnhandledMessage { .. } | Diagnostic::UnknownCapsEvent { .. } => {
                if !tracing::enabled!(tracing::Level::DEBUG) {
                    continue;
                }
                let line = diagnostic.to_string();
                // Past the cap the set stops growing and the rule degrades to
                // logging every occurrence — noisy, but only at a level the
                // developer asked for, and never unbounded memory.
                if logged_once.len() < DIAGNOSTIC_LOG_ONCE_CAP && !logged_once.insert(line.clone())
                {
                    continue;
                }
                debug!("protocol diagnostic: {line}");
            }
            Diagnostic::DecodeFailed { .. } => {
                warn!("protocol diagnostic: {diagnostic}");
                if let Some(dump) = diagnostic.hexdump() {
                    debug!("the bytes that failed to decode:\n{dump}");
                }
            }
            Diagnostic::CapsDecodeFailed { .. } => {
                warn!("protocol diagnostic: {diagnostic}");
            }
            Diagnostic::ExpectedReplyMissing { request, .. } => {
                warn!("protocol diagnostic: {diagnostic}");
                if !USER_VISIBLE_REQUESTS.contains(&request.as_str()) {
                    continue;
                }
                if !off_cooldown(&mut last_raised, request.clone(), now) {
                    continue;
                }
                show.write(
                    ShowNotification::new("ViewerRequestNoReply")
                        .arg("REQUEST", request.clone())
                        // The `unique` context: one live toast per request.
                        .with_context(request.clone()),
                );
            }
            // `Diagnostic` is `#[non_exhaustive]`: a kind added upstream still
            // reaches the log rather than vanishing.
            other => warn!("protocol diagnostic: {other}"),
        }
    }
}

/// Push the collection switch to the session whenever it changes.
///
/// The plugin is added before the settings store exists, so the driver starts
/// with collection on and this corrects it on the first tick — and on every
/// later edit, from the debug-settings editor or the Advanced menu. Polling
/// [`ViewerSettings`] change detection (rather than the preferences floater's
/// apply hook) is what makes the raw editor work as a writer.
pub fn apply_diagnostics_setting(
    settings: Res<ViewerSettings>,
    mut sl: MessageWriter<SlCommand>,
    mut pushed: Local<Option<bool>>,
) {
    if !settings.is_changed() && pushed.is_some() {
        return;
    }
    let enabled = settings
        .store()
        .get_bool(SETTING_COLLECT_DIAGNOSTICS)
        .unwrap_or(true);
    if *pushed == Some(enabled) {
        return;
    }
    *pushed = Some(enabled);
    sl.write(SlCommand(Command::SetDiagnostics(enabled)));
}

/// **The demo source** (viewer-only, gated on [`DEMO_ENV`]): raise a staggered
/// spread of sample notifications so the live stacking / timeout / fade / modal
/// behaviour can be watched without a server alert. A no-op unless the env var
/// is set.
///
/// Two stages, spaced in time: first the four **corner** toasts (after a short
/// settle so the Fluent bundle has loaded and their text resolves through i18n
/// rather than echoing raw keys), then a few seconds later the centred **modal**
/// — so the corner stack is seen in its corner before the modal's scrim covers
/// it. Bodies are resolved through [`Translator`] so they localise like the rest
/// of the UI.
pub fn spawn_notification_demo(
    time: Res<Time>,
    translator: Translator,
    mut show: MessageWriter<ShowNotification>,
    mut stage: Local<u8>,
    mut elapsed: Local<f32>,
) {
    if std::env::var_os(DEMO_ENV).is_none() || *stage >= 2 {
        return;
    }
    *elapsed += time.delta_secs();
    if *stage == 0 {
        // Let the i18n bundle (an async asset) and the login settle, so the toast
        // text resolves through Fluent rather than falling back to its raw key.
        if *elapsed < DEMO_START_DELAY_SECS {
            return;
        }
        show.write(
            ShowNotification::new("SystemTip").with_body(translator.get("notification-demo-tip")),
        );
        show.write(
            ShowNotification::new("SystemMessage")
                .with_body(translator.get("notification-demo-notify")),
        );
        show.write(
            ShowNotification::new("RegionRestartMinutes")
                .arg("MINUTES", "5")
                // The reference dedups a restart countdown per region; the context
                // scopes the `unique` so two regions' restarts coexist.
                .with_context("demo-region"),
        );
        show.write(
            ShowNotification::new("GenericAlert")
                .with_body(translator.get("notification-demo-alert")),
        );
        *stage = 1;
        *elapsed = 0.0;
        return;
    }
    if *elapsed >= DEMO_MODAL_DELAY_SECS {
        show.write(ShowNotification::new("ConfirmQuit"));
        *stage = 2;
    }
}

/// The gallery / test-harness specimen (`viewer-ui-test-harness`): a static
/// alert-style toast card — a paragraph body, an OK / Cancel button row and the
/// ignore checkbox — built through the same `build_toast_card` the live host
/// uses, minus the handlers (its buttons emit an inert [`UiAction`]).
pub fn spawn_notification_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = ToastContent {
        kind: NotificationKind::Alert,
        // A title so the layout matrix sweeps the header line too.
        title: Some(cx.text("Region restart")),
        body: cx.text(SPECIMEN_BODY),
        buttons: vec![
            ToastButtonSpec {
                name: "OK",
                label: cx.text("OK"),
                is_default: true,
            },
            ToastButtonSpec {
                name: "Cancel",
                label: cx.text("Cancel"),
                is_default: false,
            },
        ],
        ignorable: true,
        ignore_label: cx.text("Don't show me this again"),
        closable: true,
        font_size: cx.font_size,
        // The input field rides the specimen so the harness's layout matrix
        // sweeps the save-outfit-style prompt (body + field + buttons).
        input: Some(cx.text("My Outfit (new)")),
    };
    let card = build_toast_card(commands, &content);
    commands.entity(card.root).insert(ChildOf(parent));
    // The close × is wired inert in the specimen (an action like the buttons).
    if let Some(close) = card.close {
        commands
            .entity(close)
            .insert((Button, TabIndex(0)))
            .observe(
                move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                    actions.write(UiAction {
                        element: NOTIFICATION_ELEMENT,
                        action: "close",
                    });
                },
            );
    }
    for (index, (button, name)) in card.buttons.iter().enumerate() {
        let name = *name;
        let tab = i32::try_from(index).unwrap_or(0).saturating_add(1);
        commands
            .entity(*button)
            .insert((Button, TabIndex(tab)))
            .observe(
                move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                    actions.write(UiAction {
                        element: NOTIFICATION_ELEMENT,
                        action: name,
                    });
                },
            );
    }
    card.root
}

/// The specimen's body prose — a paragraph long enough to force the wrap the
/// matrix sweeps.
const SPECIMEN_BODY: &str = "The region you are in now will restart in 5 minutes. If you stay in \
    this region you will be logged out until the restart is complete.";

#[cfg(test)]
mod tests {
    use bevy::prelude::{App, Messages, Update};
    use bevy::text::EditableText;
    use pretty_assertions::assert_eq;
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use bevy::time::Time;
    use sl_client_bevy::{
        Command, Diagnostic, MessageId, SessionError, SlCommand, SlCommandFailed, SlDiagnostic,
        WireError,
    };

    use super::{
        IgnoreCheckbox, ResolveNotification, SETTING_COLLECT_DIAGNOSTICS, Toast,
        announce_command_failures, apply_diagnostics_setting, auto_response_button,
        ingest_protocol_diagnostics, resolve_notifications,
    };
    use crate::notifications::{
        NOTIFICATIONS, NotificationButton, NotificationIgnore, NotificationKind,
        NotificationManager, NotificationPriority, NotificationResponse, NotificationTemplate,
        ShowNotification, last_response_setting_name,
    };
    use crate::settings::ViewerSettings;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A two-button form for the synthetic templates: "OK" default, "Cancel".
    const TEST_FORM: &[NotificationButton] = &[
        NotificationButton {
            name: "OK",
            label_key: "k-ok",
            is_default: true,
        },
        NotificationButton {
            name: "Cancel",
            label_key: "k-cancel",
            is_default: false,
        },
    ];

    /// A synthetic template with the given ignore kind over [`TEST_FORM`].
    const fn synthetic(ignore: NotificationIgnore) -> NotificationTemplate {
        NotificationTemplate {
            name: "SyntheticIgnoreProbe",
            kind: NotificationKind::Alert,
            message_key: "k-body",
            title_key: None,
            priority: NotificationPriority::Normal,
            persist: false,
            log_to_chat: false,
            unique: false,
            ignore,
            ignore_key: Some("k-ignore"),
            form: TEST_FORM,
            input: None,
        }
    }

    /// The default-response kinds auto-answer with the form's default button;
    /// show-again answers nothing — the reference `handleIgnoredNotification`
    /// per kind, with no store involved.
    #[test]
    fn suppressed_kinds_pick_their_auto_response() {
        let default = synthetic(NotificationIgnore::DefaultResponse);
        assert_eq!(auto_response_button(&default, None), Some("OK"));
        let session = synthetic(NotificationIgnore::DefaultResponseSessionOnly);
        assert_eq!(auto_response_button(&session, None), Some("OK"));
        let show_again = synthetic(NotificationIgnore::ShowAgain);
        assert_eq!(auto_response_button(&show_again, None), None);
    }

    /// A last-response template replays the saved button; an unsaved (empty)
    /// or no-longer-matching saved name falls back to the default button.
    #[test]
    fn last_response_replays_the_saved_button() -> Result<(), TestError> {
        let tmpl = synthetic(NotificationIgnore::LastResponse);
        let setting = last_response_setting_name(tmpl.name);
        let mut store = SettingsStore::new();
        store.register(&setting, SettingValue::String(String::new()), "saved")?;
        let mut settings = ViewerSettings::from_store_for_test(store);
        // Unsaved (the registered empty default): the default button.
        assert_eq!(auto_response_button(&tmpl, Some(&settings)), Some("OK"));
        // Saved and still a form button: replayed.
        settings.set_account(&setting, SettingValue::String("Cancel".to_owned()));
        assert_eq!(auto_response_button(&tmpl, Some(&settings)), Some("Cancel"));
        // Saved but no longer a form button (a form edit since): the default.
        settings.set_account(&setting, SettingValue::String("Gone".to_owned()));
        assert_eq!(auto_response_button(&tmpl, Some(&settings)), Some("OK"));
        Ok(())
    }

    /// Drive [`resolve_notifications`] through a throwaway app against a toast
    /// whose [`Toast::input_field`] is a real [`EditableText`] holding
    /// "Renamed Outfit", resolving it with `button`, and return the single
    /// response written (`None` if none was) — the host-level round trip that
    /// a form field's edited text reaches [`NotificationResponse::input`].
    fn resolve_with_field(button: Option<&'static str>) -> Option<NotificationResponse> {
        let mut app = App::new();
        app.add_message::<ResolveNotification>();
        app.add_message::<NotificationResponse>();
        app.init_resource::<NotificationManager>();
        app.add_systems(Update, resolve_notifications);
        let field = app
            .world_mut()
            .spawn(EditableText::new("Renamed Outfit"))
            .id();
        let id = app
            .world_mut()
            .resource_mut::<NotificationManager>()
            .allocate_id();
        let toast = app
            .world_mut()
            .spawn(Toast {
                id,
                template: "RenameOutfit",
                priority: NotificationPriority::Normal,
                default_button: Some("OK"),
                age: 0.0,
                lifetime: 0.0,
                opacity: 1.0,
                hovered: false,
                overflowed: false,
                resolved: false,
                input_field: Some(field),
            })
            .id();
        app.world_mut()
            .write_message(ResolveNotification { toast, button });
        app.update();
        let messages = app.world().resource::<Messages<NotificationResponse>>();
        let mut cursor = messages.get_cursor();
        cursor.read(messages).next().cloned()
    }

    /// Drive [`resolve_notifications`] against a real catalogue template whose
    /// toast carries a **ticked** ignore checkbox, over a store pre-seeded by
    /// `register`, and return the app for store inspection.
    fn resolve_ticked(
        template: &'static str,
        button: Option<&'static str>,
        register: impl FnOnce(&mut SettingsStore),
    ) -> App {
        let mut store = SettingsStore::new();
        register(&mut store);
        let mut app = App::new();
        app.add_message::<ResolveNotification>();
        app.add_message::<NotificationResponse>();
        app.init_resource::<NotificationManager>();
        app.insert_resource(ViewerSettings::from_store_for_test(store));
        app.add_systems(Update, resolve_notifications);
        let checkbox = app.world_mut().spawn(IgnoreCheckbox { checked: true }).id();
        let id = app
            .world_mut()
            .resource_mut::<NotificationManager>()
            .allocate_id();
        let toast = app
            .world_mut()
            .spawn(Toast {
                id,
                template,
                priority: NotificationPriority::Normal,
                default_button: Some("OK"),
                age: 0.0,
                lifetime: 0.0,
                opacity: 1.0,
                hovered: false,
                overflowed: false,
                resolved: false,
                input_field: None,
            })
            .add_child(checkbox)
            .id();
        app.world_mut()
            .write_message(ResolveNotification { toast, button });
        app.update();
        app
    }

    /// The account-scope override a test's store holds for `name`, if any.
    fn account_override(app: &App, name: &str) -> Option<SettingValue> {
        app.world()
            .resource::<ViewerSettings>()
            .store()
            .get_override(Scope::Account, name)
            .cloned()
    }

    /// Ticking the box on a default-response template records the suppression
    /// (the account-scope `Bool(false)` the raise path honours).
    #[test]
    fn ticked_default_response_records_a_suppression() -> Result<(), TestError> {
        let tmpl = NOTIFICATIONS
            .iter()
            .find(|entry| entry.ignore == NotificationIgnore::DefaultResponse)
            .ok_or("no DefaultResponse template in the catalogue")?;
        let app = resolve_ticked(tmpl.name, None, |store| {
            store
                .register(tmpl.name, SettingValue::Bool(true), "show")
                .ok();
        });
        assert_eq!(
            account_override(&app, tmpl.name),
            Some(SettingValue::Bool(false))
        );
        Ok(())
    }

    /// Ticking the box on a last-response template records the suppression
    /// *and* the chosen button, so the next raise replays it.
    #[test]
    fn ticked_last_response_saves_the_chosen_button() -> Result<(), TestError> {
        let tmpl = NOTIFICATIONS
            .iter()
            .find(|entry| entry.ignore == NotificationIgnore::LastResponse)
            .ok_or("no LastResponse template in the catalogue")?;
        let chosen = tmpl
            .form
            .first()
            .map(|button| button.name)
            .ok_or("a LastResponse template needs a form")?;
        let saved_setting = last_response_setting_name(tmpl.name);
        let app = resolve_ticked(tmpl.name, Some(chosen), |store| {
            store
                .register(tmpl.name, SettingValue::Bool(true), "show")
                .ok();
            store
                .register(&saved_setting, SettingValue::String(String::new()), "saved")
                .ok();
        });
        assert_eq!(
            account_override(&app, tmpl.name),
            Some(SettingValue::Bool(false))
        );
        assert_eq!(
            account_override(&app, &saved_setting),
            Some(SettingValue::String(chosen.to_owned()))
        );
        Ok(())
    }

    /// Ticking the box on a checkbox-only template writes **no** setting — the
    /// state rides the response's `ignored` flag for the owner alone.
    #[test]
    fn ticked_checkbox_only_writes_no_setting() -> Result<(), TestError> {
        let tmpl = NOTIFICATIONS
            .iter()
            .find(|entry| entry.ignore == NotificationIgnore::CheckboxOnly)
            .ok_or("no CheckboxOnly template in the catalogue")?;
        let app = resolve_ticked(tmpl.name, None, |store| {
            store
                .register(tmpl.name, SettingValue::Bool(true), "not written")
                .ok();
        });
        assert_eq!(account_override(&app, tmpl.name), None);
        let messages = app.world().resource::<Messages<NotificationResponse>>();
        let mut cursor = messages.get_cursor();
        let response = cursor.read(messages).next().cloned();
        assert_eq!(response.map(|response| response.ignored), Some(true));
        Ok(())
    }

    /// Choosing a button submits the input field: its edited text arrives on
    /// the response alongside the button name.
    #[test]
    fn resolving_a_button_returns_the_input_fields_text() {
        let response = resolve_with_field(Some("OK"));
        assert_eq!(
            response.as_ref().and_then(|r| r.input.as_deref()),
            Some("Renamed Outfit")
        );
        assert_eq!(response.as_ref().and_then(|r| r.button), Some("OK"));
    }

    /// A dismissal (no button chosen) submits nothing: the response carries no
    /// input text even though the field exists.
    #[test]
    fn dismissing_returns_no_input() {
        let response = resolve_with_field(None);
        assert!(response.is_some(), "a dismissal still writes a response");
        assert_eq!(response.and_then(|r| r.input), None);
    }

    /// An app wired for [`announce_command_failures`] alone.
    fn command_failure_app() -> App {
        let mut app = App::new();
        app.add_message::<SlCommandFailed>();
        app.add_message::<ShowNotification>();
        app.init_resource::<Time>();
        app.add_systems(Update, announce_command_failures);
        app
    }

    /// Drain and return the frame's [`ShowNotification`]s. Draining keeps the
    /// count exact across several `update()`s, which a fresh cursor would not
    /// (the double buffer drops the oldest frame).
    fn drain_raises(app: &mut App) -> Vec<ShowNotification> {
        app.world_mut()
            .resource_mut::<Messages<ShowNotification>>()
            .drain()
            .collect()
    }

    /// A failed command becomes a toast naming the action and the reason, with
    /// the command name as the `unique` context — the whole point of
    /// [`SlCommandFailed`]: an action that did nothing says so.
    #[test]
    fn a_failed_command_raises_a_named_notification() -> Result<(), TestError> {
        let mut app = command_failure_app();
        app.world_mut().write_message(SlCommandFailed {
            command: "DeleteObjects",
            error: SessionError::NoCircuit,
        });
        app.update();
        let raises = drain_raises(&mut app);
        assert_eq!(raises.len(), 1, "one raise for one failure");
        let raise = raises.first().ok_or("no raise was written")?;
        assert_eq!(raise.template, "ViewerCommandSendFailed");
        assert_eq!(raise.context.as_deref(), Some("DeleteObjects"));
        let args: Vec<(&str, &str)> = raise
            .args
            .pairs()
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        assert!(
            args.contains(&("COMMAND", "DeleteObjects")),
            "the toast names the action: {args:?}"
        );
        assert!(
            args.iter()
                .any(|(key, value)| *key == "REASON" && !value.is_empty()),
            "the reason is rendered from the error: {args:?}"
        );
        Ok(())
    }

    /// A per-frame command (camera, controls) fails every frame while the
    /// circuit is down; only the first raise inside the cooldown reaches the
    /// screen, and a *different* command is never suppressed by it.
    #[test]
    fn repeat_failures_are_coalesced_per_command() {
        let mut app = command_failure_app();
        let mut raised = 0;
        for _tick in 0..3 {
            app.world_mut().write_message(SlCommandFailed {
                command: "SetCamera",
                error: SessionError::NoCircuit,
            });
            app.update();
            raised += drain_raises(&mut app).len();
        }
        assert_eq!(raised, 1, "the repeats are coalesced");
        app.world_mut().write_message(SlCommandFailed {
            command: "DeleteObjects",
            error: SessionError::NoCircuit,
        });
        app.update();
        assert_eq!(
            drain_raises(&mut app).len(),
            1,
            "another command has its own cooldown"
        );
    }

    /// An app wired for [`ingest_protocol_diagnostics`] alone.
    fn diagnostic_app() -> App {
        let mut app = App::new();
        app.add_message::<SlDiagnostic>();
        app.add_message::<ShowNotification>();
        app.init_resource::<Time>();
        app.add_systems(Update, ingest_protocol_diagnostics);
        app
    }

    /// Feed one diagnostic and return what reached the screen that frame.
    fn raises_for(app: &mut App, diagnostic: Diagnostic) -> Vec<ShowNotification> {
        app.world_mut().write_message(SlDiagnostic(diagnostic));
        app.update();
        drain_raises(app)
    }

    /// A missing reply for something the user asked for reaches the screen
    /// naming the request — the one diagnostic class a user can feel.
    #[test]
    fn a_missing_reply_raises_a_named_notification() -> Result<(), TestError> {
        let mut app = diagnostic_app();
        let raises = raises_for(
            &mut app,
            Diagnostic::ExpectedReplyMissing {
                request: Diagnostic::SIT_REQUEST.to_owned(),
                sequence: None,
            },
        );
        assert_eq!(raises.len(), 1, "one raise for one missing reply");
        let raise = raises.first().ok_or("no raise was written")?;
        assert_eq!(raise.template, "ViewerRequestNoReply");
        assert_eq!(raise.context.as_deref(), Some("Sit"));
        let args: Vec<(&str, &str)> = raise
            .args
            .pairs()
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        assert!(
            args.contains(&("REQUEST", "Sit")),
            "the toast names the request: {args:?}"
        );
        Ok(())
    }

    /// The missing replies that are *not* the user's business stay off the
    /// screen: a logout (already surfaced as the logout), a capability the grid
    /// does not implement at all (stock OpenSim never serves
    /// `SimulatorFeatures`, so this fires on every login there), and a reliable
    /// packet that died with the circuit (the disconnect is the news).
    #[test]
    fn background_missing_replies_are_not_raised() {
        let mut app = diagnostic_app();
        for (request, sequence) in [
            (Diagnostic::LOGOUT_REQUEST.to_owned(), None),
            ("SimulatorFeatures".to_owned(), None),
            (
                "AgentThrottle".to_owned(),
                Some(sl_client_bevy::SequenceNumber(7)),
            ),
        ] {
            let raises = raises_for(
                &mut app,
                Diagnostic::ExpectedReplyMissing { request, sequence },
            );
            assert!(raises.is_empty(), "only agent operations reach the screen");
        }
    }

    /// The developer-facing variants are logged, never raised: an unmodelled
    /// message or a decode failure is not something to interrupt a user with.
    #[test]
    fn developer_diagnostics_stay_out_of_the_ui() {
        let mut app = diagnostic_app();
        for diagnostic in [
            Diagnostic::UnhandledMessage {
                id: MessageId::High(1),
                name: "SomeMessage",
                child: false,
            },
            Diagnostic::UnknownCapsEvent {
                message: "WeirdEvent".to_owned(),
            },
            Diagnostic::CapsDecodeFailed {
                message: "ParcelProperties".to_owned(),
                reason: Some("missing field".to_owned()),
            },
            Diagnostic::DecodeFailed {
                id: MessageId::High(1),
                name: None,
                error: WireError::UnexpectedEof {
                    needed: 4,
                    available: 1,
                },
                raw: vec![0x01, 0x02],
                failed_offset: 1,
            },
        ] {
            let raises = raises_for(&mut app, diagnostic);
            assert!(raises.is_empty(), "developer diagnostics are log-only");
        }
    }

    /// A user retrying the same action against an unresponsive simulator gets
    /// one toast, not one per attempt.
    #[test]
    fn repeat_missing_replies_are_coalesced_per_request() {
        let mut app = diagnostic_app();
        let mut raised = 0;
        for _attempt in 0..3 {
            raised += raises_for(
                &mut app,
                Diagnostic::ExpectedReplyMissing {
                    request: Diagnostic::SIT_REQUEST.to_owned(),
                    sequence: None,
                },
            )
            .len();
        }
        assert_eq!(raised, 1, "the repeats are coalesced");
    }

    /// Every allowlisted label is one the catalogue template can actually
    /// render, and the allowlist never quietly grows to include the background
    /// traffic the live grid produces on every login.
    #[test]
    fn only_agent_operations_are_user_visible() {
        assert_eq!(
            super::USER_VISIBLE_REQUESTS,
            [Diagnostic::SIT_REQUEST],
            "widening this list needs the three tests in its doc comment to hold"
        );
    }

    /// The switch is pushed once at startup and again on every change, and
    /// never re-sent for a write that left it alone — the session must not be
    /// told to reconfigure on every settings edit.
    #[test]
    fn the_collection_switch_is_pushed_on_change_only() -> Result<(), TestError> {
        let mut store = SettingsStore::new();
        store.register(
            SETTING_COLLECT_DIAGNOSTICS,
            SettingValue::Bool(true),
            "collect",
        )?;
        let mut app = App::new();
        app.add_message::<SlCommand>();
        app.insert_resource(ViewerSettings::from_store_for_test(store));
        app.add_systems(Update, apply_diagnostics_setting);

        app.update();
        assert_eq!(
            pushed_diagnostics(&mut app),
            vec![true],
            "the startup value is pushed once"
        );
        app.update();
        assert!(
            pushed_diagnostics(&mut app).is_empty(),
            "an unchanged tick pushes nothing"
        );

        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .set_account(SETTING_COLLECT_DIAGNOSTICS, SettingValue::Bool(false));
        app.update();
        assert_eq!(
            pushed_diagnostics(&mut app),
            vec![false],
            "turning it off is pushed"
        );

        // A settings write that does not touch this value marks the resource
        // changed but must not re-push.
        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .set_account(SETTING_COLLECT_DIAGNOSTICS, SettingValue::Bool(false));
        app.update();
        assert!(
            pushed_diagnostics(&mut app).is_empty(),
            "re-writing the same value pushes nothing"
        );
        Ok(())
    }

    /// Drain the frame's `SetDiagnostics` pushes.
    fn pushed_diagnostics(app: &mut App) -> Vec<bool> {
        app.world_mut()
            .resource_mut::<Messages<SlCommand>>()
            .drain()
            .filter_map(|command| match command.0 {
                Command::SetDiagnostics(enabled) => Some(enabled),
                _other => None,
            })
            .collect()
    }
}
