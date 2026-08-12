//! The **teleport progress overlay** — a centred panel that tracks a teleport
//! from request to arrival, and (the point of this module) resolves every
//! teleport to a clear success **or** failure end state instead of the
//! reference viewer's occasional silent hang.
//!
//! # What it shows
//!
//! A teleport is a multi-step protocol handshake ([`Event::TeleportStarted`] →
//! [`Event::TeleportProgress`] → [`Event::TeleportFinished`] →
//! [`Event::RegionChanged`], or the intra-region [`Event::TeleportLocal`], or the
//! [`Event::TeleportFailed`] error path). The overlay renders the live phase, the
//! elapsed time, the destination (when the initiating surface supplies one), and
//! the simulator's progress messages — more than the reference's opaque progress
//! screen surfaces.
//!
//! # Why a client watchdog
//!
//! The reference viewer can leave a teleport progress screen up forever when the
//! terminal message never arrives. The [`Session`](sl_client_bevy) already arms a
//! 30 s server-timeout that emits [`Event::TeleportFailed`], but a *lost* event
//! (a dropped packet on the failure path) would still hang the UI. So this module
//! adds a **client-side watchdog** on top: a soft threshold that flags a
//! slow-but-live teleport (offering Cancel), and a hard backstop that — if no
//! terminal event has arrived well past the server timeout — force-resolves the
//! overlay to a failure *and* sends [`Command::CancelTeleport`] so the session
//! returns to `Active` and the user is never stuck unable to teleport again.
//!
//! # The shared entry point
//!
//! Any teleport surface can call [`issue_teleport`] to fire the teleport **and**
//! open the overlay with a destination label and a working Retry button in one
//! call — the "one backend, many surfaces" the double-click / minimap / world-map
//! teleports share. Surfaces that write [`Command::Teleport`] directly still get
//! the overlay (the event path opens it), just without the pre-filled label.
//!
//! Reference (Firestorm, read-only): `llstartup` (the teleport progress screen),
//! `llagent` / `llviewermessage` (`TeleportStart` / `TeleportProgress` /
//! `TeleportLocal` / `TeleportFinish` / `TeleportFailed` handling).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use sl_client_bevy::{
    Command, RegionCoordinates, RegionHandle, SlCommand, SlEvent, SlSessionEvent, Vector,
};

use crate::ui::{UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;

/// Seconds a live teleport may run before the overlay flags it as slow (and
/// nudges the user that they can cancel). Below the server's 30 s timeout so the
/// warning appears *before* the automatic failure, not after.
const SOFT_WATCHDOG_SECS: f64 = 18.0;

/// Seconds a live teleport may run before the client **force-resolves** it to a
/// failure and sends [`Command::CancelTeleport`]. A backstop above the session's
/// 30 s server timeout: it only ever fires if the terminal [`Event::TeleportFailed`]
/// itself was lost, so a hung teleport can never outlast it.
const HARD_WATCHDOG_SECS: f64 = 38.0;

/// Seconds a successful-arrival confirmation lingers before the overlay hides
/// itself, so an arrival reads as a brief acknowledgement rather than a modal.
const SUCCESS_LINGER_SECS: f64 = 2.5;

/// The overlay panel background (a dark, slightly translucent slate).
const PANEL_BG: Color = Color::srgba(0.09, 0.10, 0.13, 0.94);

/// The overlay panel border.
const PANEL_BORDER: Color = Color::srgb(0.30, 0.34, 0.42);

/// The colour of the title while a teleport is in progress.
const TITLE_ACTIVE: Color = Color::srgb(0.85, 0.89, 0.96);

/// The colour of the title on a successful arrival.
const TITLE_SUCCESS: Color = Color::srgb(0.55, 0.85, 0.55);

/// The colour of the title on a failed teleport.
const TITLE_FAILED: Color = Color::srgb(0.95, 0.55, 0.50);

/// The colour of the slow-teleport warning line.
const WARN: Color = Color::srgb(0.95, 0.78, 0.40);

/// The muted colour of the detail / status lines.
const DETAIL: Color = Color::srgb(0.72, 0.76, 0.82);

/// A cancel / dismiss button's resting background.
const BUTTON_BG: Color = Color::srgb(0.22, 0.25, 0.31);

/// The Retry button's background — the toolbar's lit blue, so it reads as the
/// affirmative recovery action.
const RETRY_BG: Color = Color::srgb(0.22, 0.40, 0.60);

/// A concrete teleport target, kept so the overlay's **Retry** button can
/// re-issue the exact same teleport after a failure.
#[derive(Debug, Clone)]
pub(crate) struct TeleportTarget {
    /// The destination region handle.
    pub(crate) region_handle: RegionHandle,
    /// The destination region-local arrival position.
    pub(crate) position: RegionCoordinates,
    /// The arrival look-at direction.
    pub(crate) look_at: Vector,
}

/// A request to open the teleport overlay for a teleport this frame's surface is
/// initiating. Emitting it is optional — the overlay also opens from the incoming
/// teleport events — but it lets a surface pre-fill the destination label and
/// enable Retry. Prefer the [`issue_teleport`] helper, which writes this and the
/// [`Command::Teleport`] together.
#[derive(Message, Debug, Clone)]
pub(crate) struct BeginTeleportFlow {
    /// A human-readable destination label (e.g. a region name or `Region (128, 128)`),
    /// shown on the overlay. `None` leaves the destination line blank.
    pub(crate) destination: Option<String>,
    /// The target to re-issue if the user hits Retry. `None` (landmark / lure
    /// teleports, whose destination is not known until arrival) disables Retry.
    pub(crate) retry: Option<TeleportTarget>,
}

/// Fire a location teleport **and** open the progress overlay in one call: writes
/// [`Command::Teleport`] and a [`BeginTeleportFlow`] carrying the destination
/// label and a Retry payload. The shared entry point every location-teleport
/// surface (double-click, minimap, world map) routes through.
pub(crate) fn issue_teleport(
    commands: &mut MessageWriter<SlCommand>,
    begin: &mut MessageWriter<BeginTeleportFlow>,
    target: TeleportTarget,
    destination: Option<String>,
) {
    begin.write(BeginTeleportFlow {
        destination,
        retry: Some(target.clone()),
    });
    commands.write(SlCommand(Command::Teleport {
        region_handle: target.region_handle,
        position: target.position,
        look_at: target.look_at,
    }));
}

/// Which phase of the handshake the teleport is currently in, for the status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The request was sent; awaiting the first server acknowledgement.
    Requested,
    /// The simulator is reporting progress (`TeleportProgress`).
    InProgress,
    /// The destination was resolved (`TeleportFinish`); the circuit handover to
    /// the new region is underway, awaiting its handshake.
    Arriving,
}

/// How a teleport ended, once it is no longer pending.
#[derive(Debug, Clone)]
enum Outcome {
    /// Still running.
    Pending,
    /// Arrived successfully (intra-region `TeleportLocal`, or a cross-region
    /// `RegionChanged`).
    Succeeded,
    /// Failed — carries the server reason and any extra alert detail.
    Failed {
        /// The primary failure reason (server message / key, or a client message
        /// for the watchdog backstop).
        reason: String,
        /// Extra detail (an `AlertInfo` key's parameters), if any.
        detail: Option<String>,
    },
}

/// The single in-flight (or just-resolved) teleport the overlay renders.
#[derive(Debug, Clone)]
struct Entry {
    /// The viewer clock time (seconds) the flow began.
    started_at: f64,
    /// The viewer clock time (seconds) of the last state change, for elapsed
    /// display and the success-linger timer.
    updated_at: f64,
    /// The current handshake phase.
    phase: Phase,
    /// The last progress message the simulator sent, if any.
    message: Option<String>,
    /// The destination label a surface supplied, if any.
    destination: Option<String>,
    /// How the teleport ended (or [`Outcome::Pending`] while running).
    outcome: Outcome,
    /// Whether the soft watchdog has flagged this teleport as slow.
    stalled: bool,
    /// The target to re-issue on Retry, if known.
    retry: Option<TeleportTarget>,
}

/// The teleport-flow state the overlay renders: at most one teleport is tracked
/// at a time (a new one replaces the last).
#[derive(Resource, Debug, Default)]
struct TeleportFlow {
    /// The current teleport, or `None` when the overlay is idle/hidden.
    entry: Option<Entry>,
}

impl TeleportFlow {
    /// Begin (or restart) a pending teleport at `now`, carrying an optional
    /// destination label and Retry target.
    fn begin(&mut self, now: f64, destination: Option<String>, retry: Option<TeleportTarget>) {
        self.entry = Some(Entry {
            started_at: now,
            updated_at: now,
            phase: Phase::Requested,
            message: None,
            destination,
            outcome: Outcome::Pending,
            stalled: false,
            retry,
        });
    }

    /// Ensure a pending entry exists (opening a bare one at `now` if a teleport
    /// event arrived without a preceding [`BeginTeleportFlow`]), then return it.
    fn pending_entry(&mut self, now: f64) -> &mut Entry {
        if !matches!(
            self.entry.as_ref().map(|entry| &entry.outcome),
            Some(Outcome::Pending)
        ) {
            self.begin(now, None, None);
        }
        // Just ensured above (the fallback is never actually inserted).
        self.entry.get_or_insert(Entry {
            started_at: now,
            updated_at: now,
            phase: Phase::Requested,
            message: None,
            destination: None,
            outcome: Outcome::Pending,
            stalled: false,
            retry: None,
        })
    }
}

/// A marker on the overlay's root container.
#[derive(Component)]
struct OverlayRoot;

/// A marker on the overlay's title text.
#[derive(Component)]
struct OverlayTitle;

/// A marker on the overlay's status (phase) line.
#[derive(Component)]
struct OverlayStatus;

/// A marker on the overlay's detail (destination + elapsed) line.
#[derive(Component)]
struct OverlayDetail;

/// A marker on the overlay's message / warning line.
#[derive(Component)]
struct OverlayMessage;

/// Which action an overlay button performs.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayButton {
    /// Cancel the in-progress teleport ([`Command::CancelTeleport`]).
    Cancel,
    /// Dismiss the overlay after a terminal outcome.
    Dismiss,
    /// Re-issue the last teleport after a failure.
    Retry,
}

/// The teleport progress overlay plugin: registers the flow resource and the
/// [`BeginTeleportFlow`] message, spawns the (hidden) overlay once, and keeps it
/// current from the teleport events and the watchdog.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TeleportProgressPlugin;

impl Plugin for TeleportProgressPlugin {
    /// Wire the resource, message, spawn, and the per-frame ingest / watchdog /
    /// render systems.
    fn build(&self, app: &mut App) {
        app.init_resource::<TeleportFlow>()
            .add_message::<BeginTeleportFlow>()
            .add_systems(Startup, spawn_overlay.after(UiScaffoldSystems::SpawnRoot))
            .add_systems(
                Update,
                (ingest_begin, ingest_events, watchdog, render_overlay).chain(),
            );
    }
}

/// Spawn the hidden overlay: a full-screen, non-blocking centring container with
/// a single panel of text lines and a button row, built **once** and updated in
/// place thereafter (never despawned/respawned).
fn spawn_overlay(mut commands: Commands, root: Res<UiRoot>) {
    let overlay = commands
        .spawn((
            OverlayRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                // A little below the top edge, so it does not fight the top bar.
                align_items: AlignItems::Center,
                padding: UiRect::top(Val::Percent(12.0)),
                ..default()
            },
            // Never blocks the world: only the panel itself takes clicks.
            Pickable::IGNORE,
            GlobalZIndex(900),
            Visibility::Hidden,
            Name::new("teleport-overlay"),
            ChildOf(root.0),
        ))
        .id();

    let panel = commands
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(18.0), Val::Px(14.0)),
                border: UiRect::all(Val::Px(1.0)),
                max_width: Val::Px(440.0),
                align_items: AlignItems::Stretch,
                // The panel sits at the top of the centring container.
                align_self: AlignSelf::FlexStart,
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..column(Val::Px(6.0))
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(PANEL_BORDER),
            Name::new("teleport-overlay-panel"),
            ChildOf(overlay),
        ))
        .id();

    commands.spawn((
        OverlayTitle,
        Text::new(""),
        UiFont::Sans.at(16.0),
        TextColor(TITLE_ACTIVE),
        TextLayout::no_wrap(),
        ChildOf(panel),
    ));
    commands.spawn((
        OverlayStatus,
        Text::new(""),
        UiFont::Sans.at(13.0),
        TextColor(DETAIL),
        ChildOf(panel),
    ));
    commands.spawn((
        OverlayDetail,
        Text::new(""),
        UiFont::Sans.at(12.0),
        TextColor(DETAIL),
        ChildOf(panel),
    ));
    commands.spawn((
        OverlayMessage,
        Text::new(""),
        UiFont::Sans.at(12.0),
        TextColor(WARN),
        ChildOf(panel),
    ));

    let button_row = commands
        .spawn((
            Node {
                margin: UiRect::top(Val::Px(4.0)),
                justify_content: JustifyContent::FlexEnd,
                ..row(Val::Px(8.0))
            },
            Name::new("teleport-overlay-buttons"),
            ChildOf(panel),
        ))
        .id();

    spawn_button(
        &mut commands,
        button_row,
        OverlayButton::Retry,
        "Retry",
        RETRY_BG,
    );
    spawn_button(
        &mut commands,
        button_row,
        OverlayButton::Cancel,
        "Cancel",
        BUTTON_BG,
    );
    spawn_button(
        &mut commands,
        button_row,
        OverlayButton::Dismiss,
        "Dismiss",
        BUTTON_BG,
    );
}

/// Spawn one overlay button of `kind` with `label`, hidden until its outcome
/// calls for it.
fn spawn_button(
    commands: &mut Commands,
    row: Entity,
    kind: OverlayButton,
    label: &str,
    background: Color,
) {
    commands
        .spawn((
            Button,
            TabIndex(0),
            kind,
            Node {
                padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(PANEL_BORDER),
            BackgroundColor(background),
            Visibility::Hidden,
            Name::new(match kind {
                OverlayButton::Cancel => "teleport-button:cancel",
                OverlayButton::Dismiss => "teleport-button:dismiss",
                OverlayButton::Retry => "teleport-button:retry",
            }),
            ChildOf(row),
        ))
        .with_child((
            Text::new(label.to_owned()),
            UiFont::Sans.at(13.0),
            TextColor(Color::WHITE),
            TextLayout::no_wrap(),
        ))
        .observe(on_overlay_button);
}

/// Open the overlay from an initiating surface's [`BeginTeleportFlow`].
fn ingest_begin(
    time: Res<Time>,
    mut begins: MessageReader<BeginTeleportFlow>,
    mut flow: ResMut<TeleportFlow>,
) {
    let now = time.elapsed_secs_f64();
    for begin in begins.read() {
        flow.begin(now, begin.destination.clone(), begin.retry.clone());
    }
}

/// Fold the incoming teleport events into the flow state.
fn ingest_events(
    time: Res<Time>,
    mut events: MessageReader<SlEvent>,
    mut flow: ResMut<TeleportFlow>,
    mut ui_sound: MessageWriter<crate::ui_sounds::PlayUiSound>,
) {
    let now = time.elapsed_secs_f64();
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::TeleportStarted => {
                let entry = flow.pending_entry(now);
                entry.phase = Phase::Requested;
                entry.updated_at = now;
                // The reference viewer's "teleport out" chime as the flow begins.
                ui_sound.write(crate::ui_sounds::PlayUiSound(
                    crate::ui_sounds::UiSound::TeleportOut,
                ));
            }
            SlSessionEvent::TeleportProgress { message, .. } => {
                let entry = flow.pending_entry(now);
                entry.phase = Phase::InProgress;
                entry.message = (!message.is_empty()).then(|| message.clone());
                entry.updated_at = now;
            }
            SlSessionEvent::TeleportFinished { .. } => {
                let entry = flow.pending_entry(now);
                entry.phase = Phase::Arriving;
                entry.updated_at = now;
            }
            SlSessionEvent::TeleportLocal { .. } => {
                let entry = flow.pending_entry(now);
                entry.outcome = Outcome::Succeeded;
                entry.updated_at = now;
            }
            SlSessionEvent::RegionChanged { .. } => {
                // `RegionChanged` also fires for a seamless vehicle crossing; only
                // treat it as an arrival when a teleport is actually pending.
                if matches!(
                    flow.entry.as_ref().map(|entry| &entry.outcome),
                    Some(Outcome::Pending)
                ) {
                    let entry = flow.pending_entry(now);
                    entry.outcome = Outcome::Succeeded;
                    entry.updated_at = now;
                }
            }
            SlSessionEvent::TeleportFailed { reason, alert_info } => {
                let detail = alert_info.as_ref().and_then(|info| {
                    let params = info.extra_params.trim();
                    let key = info.message.trim();
                    match (key.is_empty(), params.is_empty()) {
                        (true, true) => None,
                        (false, true) => Some(key.to_owned()),
                        (true, false) => Some(params.to_owned()),
                        (false, false) => Some(format!("{key} ({params})")),
                    }
                });
                let entry = flow.pending_entry(now);
                entry.outcome = Outcome::Failed {
                    reason: if reason.trim().is_empty() {
                        "The teleport could not be completed.".to_owned()
                    } else {
                        reason.clone()
                    },
                    detail,
                };
                entry.updated_at = now;
            }
            _ => {}
        }
    }
}

/// The client watchdog: flag a slow teleport, and — well past the server timeout
/// — force a hung teleport to a failure and recover the session with
/// [`Command::CancelTeleport`] so the user is never stuck.
fn watchdog(
    time: Res<Time>,
    mut flow: ResMut<TeleportFlow>,
    mut commands: MessageWriter<SlCommand>,
) {
    let now = time.elapsed_secs_f64();
    let Some(entry) = flow.entry.as_mut() else {
        return;
    };
    if !matches!(entry.outcome, Outcome::Pending) {
        return;
    }
    let elapsed = (now - entry.started_at).max(0.0);
    if elapsed >= HARD_WATCHDOG_SECS {
        entry.outcome = Outcome::Failed {
            reason: "The teleport did not complete in time.".to_owned(),
            detail: Some(
                "No response from the destination region — the request was cancelled.".to_owned(),
            ),
        };
        entry.updated_at = now;
        // Recover the session out of its `Teleporting` state so further teleports
        // are accepted again.
        commands.write(SlCommand(Command::CancelTeleport));
    } else if elapsed >= SOFT_WATCHDOG_SECS && !entry.stalled {
        entry.stalled = true;
        entry.updated_at = now;
    }
}

/// The observer for the overlay buttons: Cancel aborts a pending teleport,
/// Dismiss clears a resolved one, Retry re-issues the last teleport.
fn on_overlay_button(
    activate: On<Activate>,
    buttons: Query<&OverlayButton>,
    mut flow: ResMut<TeleportFlow>,
    mut commands: MessageWriter<SlCommand>,
    mut begin: MessageWriter<BeginTeleportFlow>,
) {
    let Ok(kind) = buttons.get(activate.entity) else {
        return;
    };
    match kind {
        OverlayButton::Cancel => {
            commands.write(SlCommand(Command::CancelTeleport));
            flow.entry = None;
        }
        OverlayButton::Dismiss => {
            flow.entry = None;
        }
        OverlayButton::Retry => {
            let retry = flow.entry.as_ref().and_then(|entry| entry.retry.clone());
            let destination = flow
                .entry
                .as_ref()
                .and_then(|entry| entry.destination.clone());
            if let Some(target) = retry {
                issue_teleport(&mut commands, &mut begin, target, destination);
            }
        }
    }
}

/// Render the overlay from the flow state: title colour + text, status, detail,
/// message/warning line, button visibility, and the whole overlay's visibility;
/// auto-hide a success confirmation after it has lingered.
#[expect(
    clippy::too_many_arguments,
    reason = "one text/visibility query per overlay part; splitting would not simplify"
)]
#[expect(
    clippy::type_complexity,
    reason = "the disjoint per-part text/visibility queries need Without<> filters to \
              satisfy Bevy's borrow rules; a type alias per part would not read clearer"
)]
fn render_overlay(
    time: Res<Time>,
    mut flow: ResMut<TeleportFlow>,
    mut root: Query<&mut Visibility, With<OverlayRoot>>,
    mut titles: Query<(&mut Text, &mut TextColor), With<OverlayTitle>>,
    mut statuses: Query<&mut Text, (With<OverlayStatus>, Without<OverlayTitle>)>,
    mut details: Query<
        &mut Text,
        (
            With<OverlayDetail>,
            Without<OverlayTitle>,
            Without<OverlayStatus>,
        ),
    >,
    mut messages: Query<
        (&mut Text, &mut Visibility),
        (
            With<OverlayMessage>,
            Without<OverlayRoot>,
            Without<OverlayTitle>,
            Without<OverlayStatus>,
            Without<OverlayDetail>,
        ),
    >,
    mut overlay_buttons: Query<
        (&OverlayButton, &mut Visibility),
        (
            Without<OverlayRoot>,
            Without<OverlayMessage>,
            Without<OverlayTitle>,
        ),
    >,
) {
    let now = time.elapsed_secs_f64();

    // Auto-hide a lingering success confirmation.
    if let Some(entry) = flow.entry.as_ref()
        && matches!(entry.outcome, Outcome::Succeeded)
        && now - entry.updated_at >= SUCCESS_LINGER_SECS
    {
        flow.entry = None;
    }

    let Some(entry) = flow.entry.as_ref() else {
        if let Ok(mut visibility) = root.single_mut() {
            *visibility = Visibility::Hidden;
        }
        return;
    };

    if let Ok(mut visibility) = root.single_mut() {
        *visibility = Visibility::Inherited;
    }

    let elapsed = (now - entry.started_at).max(0.0);
    let (title_text, title_colour) = match &entry.outcome {
        Outcome::Pending => ("Teleporting…", TITLE_ACTIVE),
        Outcome::Succeeded => ("Arrived", TITLE_SUCCESS),
        Outcome::Failed { .. } => ("Teleport failed", TITLE_FAILED),
    };
    if let Ok((mut text, mut colour)) = titles.single_mut() {
        set_text(&mut text, title_text);
        colour.0 = title_colour;
    }

    if let Ok(mut text) = statuses.single_mut() {
        set_text(&mut text, &status_line(entry));
    }
    if let Ok(mut text) = details.single_mut() {
        set_text(&mut text, &detail_line(entry, elapsed));
    }

    // The message / warning line: the failure detail, the slow warning, or the
    // simulator's latest progress message — hidden when there is nothing to say.
    if let Ok((mut text, mut visibility)) = messages.single_mut() {
        let line = message_line(entry);
        match line {
            Some(line) => {
                set_text(&mut text, &line);
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }

    // Buttons: Cancel while pending, Dismiss + (when possible) Retry on failure,
    // nothing on a fleeting success.
    for (kind, mut visibility) in &mut overlay_buttons {
        let wanted = match kind {
            OverlayButton::Cancel => matches!(entry.outcome, Outcome::Pending),
            OverlayButton::Dismiss => matches!(entry.outcome, Outcome::Failed { .. }),
            OverlayButton::Retry => {
                matches!(entry.outcome, Outcome::Failed { .. }) && entry.retry.is_some()
            }
        };
        let next = if wanted {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != next {
            *visibility = next;
        }
    }
}

/// The status (phase) line for `entry`.
fn status_line(entry: &Entry) -> String {
    match &entry.outcome {
        Outcome::Succeeded => "You have arrived.".to_owned(),
        Outcome::Failed { .. } => "The teleport was not completed.".to_owned(),
        Outcome::Pending => match entry.phase {
            Phase::Requested => "Requesting teleport…".to_owned(),
            Phase::InProgress => "Teleport in progress…".to_owned(),
            Phase::Arriving => "Arriving at the destination region…".to_owned(),
        },
    }
}

/// The detail line (destination + elapsed) for `entry`.
fn detail_line(entry: &Entry, elapsed: f64) -> String {
    let seconds = elapsed.round().max(0.0);
    match &entry.destination {
        Some(destination) if matches!(entry.outcome, Outcome::Pending) => {
            format!("To {destination} · {seconds:.0}s")
        }
        Some(destination) => format!("To {destination}"),
        None if matches!(entry.outcome, Outcome::Pending) => format!("{seconds:.0}s elapsed"),
        None => String::new(),
    }
}

/// The message / warning line for `entry`, or `None` when there is nothing to
/// show. Failure detail wins, then the slow-teleport warning, then the
/// simulator's latest progress message.
fn message_line(entry: &Entry) -> Option<String> {
    match &entry.outcome {
        Outcome::Failed { reason, detail } => Some(match detail {
            Some(detail) => format!("{reason}\n{detail}"),
            None => reason.clone(),
        }),
        Outcome::Succeeded => None,
        Outcome::Pending => {
            if entry.stalled {
                Some("This is taking longer than usual — you can cancel and try again.".to_owned())
            } else {
                entry.message.clone()
            }
        }
    }
}

/// Set a UI [`Text`] only when it actually changes, so the overlay does not dirty
/// layout every frame (the FixedSlot / layout-gate discipline).
fn set_text(text: &mut Text, value: &str) {
    if text.0 != value {
        value.clone_into(&mut text.0);
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, Outcome, Phase, TeleportFlow, detail_line, message_line, status_line};
    use pretty_assertions::assert_eq;

    /// A pending entry begun at time 0, for the state-transition assertions.
    fn pending() -> Entry {
        Entry {
            started_at: 0.0,
            updated_at: 0.0,
            phase: Phase::Requested,
            message: None,
            destination: Some("Region (128, 128)".to_owned()),
            outcome: Outcome::Pending,
            stalled: false,
            retry: None,
        }
    }

    /// `begin` opens a pending, requested entry; `pending_entry` reuses it while
    /// pending and opens a fresh one once it has resolved.
    #[test]
    fn pending_entry_reuses_then_replaces() {
        let mut flow = TeleportFlow::default();
        flow.begin(1.0, None, None);
        // Reused while pending: same start time.
        let started = flow.pending_entry(5.0).started_at;
        assert!(
            (started - 1.0).abs() < 1.0e-9,
            "a pending entry is reused, not restarted",
        );

        // Once resolved, the next event opens a fresh entry.
        if let Some(entry) = flow.entry.as_mut() {
            entry.outcome = Outcome::Succeeded;
        }
        let restarted = flow.pending_entry(9.0).started_at;
        assert!(
            (restarted - 9.0).abs() < 1.0e-9,
            "a resolved entry is replaced by a fresh one",
        );
    }

    /// The status line follows the phase while pending and the outcome once
    /// resolved.
    #[test]
    fn status_line_tracks_phase_and_outcome() {
        let mut entry = pending();
        assert_eq!(status_line(&entry), "Requesting teleport…");
        entry.phase = Phase::Arriving;
        assert_eq!(status_line(&entry), "Arriving at the destination region…");
        entry.outcome = Outcome::Succeeded;
        assert_eq!(status_line(&entry), "You have arrived.");
    }

    /// The detail line shows the destination and a live elapsed count while
    /// pending, and drops the elapsed once resolved.
    #[test]
    fn detail_line_shows_destination_and_elapsed() {
        let entry = pending();
        assert_eq!(detail_line(&entry, 3.4), "To Region (128, 128) · 3s");
        let mut arrived = entry;
        arrived.outcome = Outcome::Succeeded;
        assert_eq!(detail_line(&arrived, 3.4), "To Region (128, 128)");
    }

    /// The message line prefers the failure detail, then the slow warning, then
    /// the progress message, and is otherwise empty.
    #[test]
    fn message_line_prioritises_failure_then_warning_then_progress() {
        let mut entry = pending();
        assert_eq!(
            message_line(&entry),
            None,
            "nothing to say while quietly pending"
        );

        entry.message = Some("Establishing connection".to_owned());
        assert_eq!(
            message_line(&entry).as_deref(),
            Some("Establishing connection"),
            "a progress message shows when present",
        );

        entry.stalled = true;
        assert!(
            message_line(&entry).is_some_and(|line| line.contains("taking longer")),
            "the slow warning outranks the progress message",
        );

        entry.outcome = Outcome::Failed {
            reason: "Region full".to_owned(),
            detail: Some("Try again shortly".to_owned()),
        };
        assert_eq!(
            message_line(&entry).as_deref(),
            Some("Region full\nTry again shortly"),
            "the failure reason + detail outrank everything",
        );
    }
}
