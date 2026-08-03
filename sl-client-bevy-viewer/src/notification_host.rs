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
//! **close ×** for early dismissal, and only [`MAX_VISIBLE_TOASTS`] show at once
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
//! registered in [`crate::ui_element::ELEMENTS`], so the host inherits the whole
//! layout matrix.
//!
//! # Timing is frame-time, not wall-clock
//!
//! A toast ages by [`Time::delta_secs`], never wall-clock, matching the chat
//! overlay ([`crate::chat`]) so a headless / manual-clock run is deterministic.

use std::cmp::Ordering;

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{SlEvent, SlSessionEvent};
use sl_settings::SettingValue;
use tracing::{debug, warn};

use crate::chat::LocalChatNotice;
use crate::i18n::Translator;
use crate::notification_persist::{PersistNotification, PersistedKind};
use crate::notifications::{
    DismissNotification, NOTIFICATIONS, NOTIFICATIONS_SECTION, NotificationKind,
    NotificationManager, NotificationPriority, NotificationRecord, NotificationResponse,
    ShowNotification, TOAST_GAP, substitute, template,
};
use crate::settings::ViewerSettings;
use crate::ui::{LogicalInset, LogicalRect, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};

/// The element id the gallery specimen and its inert actions report under.
const NOTIFICATION_ELEMENT: &str = "notification-toast";

/// The z-order the toast channel renders at — above the floaters **and** the
/// top menu / status bar ([`crate::bottom_toolbar::BOTTOM_BAR_Z`]) so a toast is
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
const DEMO_ENV: &str = "SL_VIEWER_NOTIFICATION_DEMO";

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
pub(crate) struct NotificationHostPlugin;

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

/// The channel container and its overflow control, so [`raise_notifications`]
/// can parent a corner toast to the channel and the overflow systems can drive
/// the "N more" control.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct NotificationChannelRoot {
    /// The stacking channel container the toasts are children of. Exposed so a
    /// bespoke-content toast (the group-notice card, [`crate::group_notice`]) can
    /// join the same stack — and thus the same ordering / overflow-cycling — as
    /// the catalogue toasts, via [`adopt_toast`].
    pub(crate) channel: Entity,
    /// The overflow control (a "N more ▸" cycle button), the last channel child,
    /// hidden until the stack exceeds [`MAX_VISIBLE_TOASTS`].
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
    /// Whether the toast is queued off-screen past [`MAX_VISIBLE_TOASTS`]
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
    /// The [`Toast`]-bearing entity whose opacity drives this node.
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
/// card, [`crate::group_notice`]) tears down through the same path as the
/// catalogue toasts — the reference "close counts as acknowledged".
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct ResolveNotification {
    /// The [`Toast`]-bearing entity to tear down.
    pub(crate) toast: Entity,
    /// The chosen button, or `None` for an expiry / external dismiss.
    pub(crate) button: Option<&'static str>,
}

/// Startup: declare each ignorable notification's "show again" flag (default on),
/// so a stored suppression coerces against it and the Preferences alerts tab
/// ([[viewer-preferences-alerts-tab]]) has a registered setting to bind.
fn register_notification_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    for entry in NOTIFICATIONS {
        if entry.ignorable {
            settings.register_in(
                &[NOTIFICATIONS_SECTION],
                entry.name,
                SettingValue::Bool(true),
                "Show this notification (untick to suppress it)",
            );
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
/// it from literals, so both render through the one [`build_toast_card`].
struct ToastContent {
    /// The behaviour class (drives the accent and whether it fades).
    kind: NotificationKind,
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

/// One button's resolved spec for [`build_toast_card`].
struct ToastButtonSpec {
    /// The stable button name (response id).
    name: &'static str,
    /// The resolved, display-ready label.
    label: String,
    /// Whether this is the default button.
    is_default: bool,
}

/// The entities [`build_toast_card`] produced that a caller wires: the card root,
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
        // the same measure-safe pattern as the body (see [`build_toast_card`]).
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
/// [`crate::group_notice`]) joins the catalogue toasts so it inherits the same
/// priority ordering and "N more" overflow-cycling rather than owning a second
/// channel.
///
/// Parents the card into the channel, tags it with a [`Toast`] (of the given
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
pub(crate) fn adopt_toast(
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

/// Raise the queued [`ShowNotification`]s: look up the catalogue template, honour
/// suppression and `unique` dedup, resolve the body + button labels through
/// i18n, build the toast (corner card or modal scrim) and wire its buttons and
/// ignore checkbox.
#[expect(
    clippy::too_many_arguments,
    reason = "a raise needs the request queue, the manager, the channel/root, i18n, \
              suppression settings, the dismiss channel and the persistence channel; each is \
              distinct"
)]
fn raise_notifications(
    mut commands: Commands,
    mut requests: MessageReader<ShowNotification>,
    mut manager: ResMut<NotificationManager>,
    channel: Option<Res<NotificationChannelRoot>>,
    root: Res<UiRoot>,
    translator: Translator,
    settings: Option<Res<ViewerSettings>>,
    mut dismiss: MessageWriter<DismissNotification>,
    mut chat: MessageWriter<LocalChatNotice>,
    mut persist: MessageWriter<PersistNotification>,
) {
    let Some(channel) = channel else {
        return;
    };
    for request in requests.read() {
        let Some(tmpl) = template(request.template) else {
            warn!(
                "notification: dropping raise of unknown template {}",
                request.template
            );
            continue;
        };
        if tmpl.ignorable && is_suppressed(settings.as_deref(), tmpl.name) {
            debug!(template = tmpl.name, "notification suppressed");
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
        let buttons = tmpl
            .form
            .iter()
            .map(|button| ToastButtonSpec {
                name: button.name,
                label: translator.get(button.label_key),
                is_default: button.is_default,
            })
            .collect();
        // The input field's pre-filled text resolves like the body: through
        // i18n, then `[KEY]`-substituted with the raise's args (the reference
        // defaults are templates like `[DESC] (new)`).
        let input = tmpl.input.map(|input| {
            let raw = translator.get(input.default_key);
            substitute(&raw, &request.args)
        });
        let content = ToastContent {
            kind: tmpl.kind,
            body: body.clone(),
            buttons,
            ignorable: tmpl.ignorable,
            ignore_label: translator.get("notification-ignore-checkbox"),
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
        if ignored && let Some(settings) = settings.as_deref_mut() {
            settings.set_account(toast.template, SettingValue::Bool(false));
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

/// Cap the visible stack at [`MAX_VISIBLE_TOASTS`]: show the top toasts in the
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
pub(crate) fn ingest_alert_messages(
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
pub(crate) fn spawn_notification_demo(
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
/// ignore checkbox — built through the same [`build_toast_card`] the live host
/// uses, minus the handlers (its buttons emit an inert [`UiAction`]).
pub(crate) fn spawn_notification_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = ToastContent {
        kind: NotificationKind::Alert,
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

    use super::{ResolveNotification, Toast, resolve_notifications};
    use crate::notifications::{NotificationManager, NotificationPriority, NotificationResponse};

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
}
