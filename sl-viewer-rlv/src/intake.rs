//! The **owner-say command intake** and the chat-back path: the door a worn
//! collar speaks through, and the one seam every answer leaves by.
//!
//! Everything else in the RLV family is a machine with no wires. `sl-rlv`
//! decodes the `@`-command language, holds what the held commands mean, answers
//! what a restriction allows and builds every line a script is owed — and until
//! this module existed the only thing that had ever fed it a command was the
//! RLVa console, with the agent itself as the issuer. A worn collar could not
//! restrain this viewer, because the door was not cut.
//!
//! # In: what is allowed to speak
//!
//! [`sl_viewer_world_api::rlv::swallows_owner_say`] is the whole admission
//! test, and it is deliberately loose: `CHAT_TYPE_OWNER` chat that starts with
//! `@`, while RLV is on. **The security boundary is ownership, and the
//! simulator draws it** — `llOwnerSay` reaches only the object's owner, so
//! somebody else's furniture physically cannot get here and there is nothing
//! for the viewer to re-check. A rezzed in-world prim the agent owns commands
//! the viewer directly and always has; a club's poseball cannot, which is
//! exactly why *relays* exist, and a relay is a worn object the visitor owns —
//! indistinguishable from any other down here, with the consent (ask / auto /
//! off) living in the relay script where the user installed it. There is
//! nothing to implement for relays, and the gate must stay loose enough that
//! they and owned in-world objects both get through.
//!
//! The line is then **swallowed**: it never reaches the chat overlay, the
//! Nearby transcript or the chat log, all three of which ask the same
//! predicate so the take cannot leak out of one of them.
//!
//! # Out: the one chat-back seam
//!
//! Four producers build a line and none of them can send it — the `@get*`
//! answer, the `@notify` subscribers' reports, the `@getdebug_*` answer and the
//! `@getenv_*` one. They all queue on [`RlvSession::push_reply`] and one system
//! (`drain_rlv_replies`) shouts them, because that is one send rather than four
//! and a fifth producer inherits it for free.
//!
//! # One deliberate divergence: where the debug echo goes
//!
//! With `RestrainedLoveDebug` on, the reference does **not** swallow the line:
//! it rewrites it into a nearby-chat entry reading `<object> executes: @…` and
//! shows that instead. Here the echo goes to the **RLVa console**, which is
//! where a person looking at RLV traffic already is, and the chat surfaces stay
//! clean whatever the setting says. It is the same information on a better
//! surface, and it is what gives the setting something to echo at all — until
//! now only the console processed commands, and it echoes regardless.
//!
//! # Expiry: the failure a user cannot get out of
//!
//! A worn attachment announces its own detach; an in-world object announces
//! nothing. Without a garbage collector, a prim that was derezzed or left
//! behind in another region restrains the agent **forever**, which is the one
//! failure mode of this whole family the user has no way out of. So the expiry
//! pass (`resolve_rlv_objects`) walks every restricting object against the
//! world mirror on a tick and clears the ones that are gone — see
//! [`RlvObjectWatch`] for the arithmetic and the one place it has to diverge
//! from the reference.
//!
//! Reference (Firestorm, read-only): the chat hook in `llviewermessage.cpp`
//! (~L3142, the `CHAT_TYPE_OWNER` case), `RlvHandler::processCommand` and
//! `RlvHandler::onGC` in `rlvhandler.cpp`, `RlvUtil::sendChatReply` in
//! `rlvcommon.cpp`.

use std::collections::HashMap;

use bevy::prelude::*;
use sl_client_bevy::{
    ChatChannel, ChatSource, ChatType, Command, ObjectKey, SlCommand, SlEvent, SlIdentity,
    SlSessionEvent,
};
use sl_rlv::{
    RlvBehaviour, RlvEnvSource, RlvExtSource, RlvNoFacts, RlvOutcome, RlvParam, RlvParamKind,
    RlvQuery, RlvReply, RlvState, is_valid_reply_channel, parse_chat_line,
};
use sl_viewer_notifications::ShowNotification;
use sl_viewer_settings::ViewerSettings;
use sl_viewer_world_api::rlv::{
    RlvConsoleKind, RlvEnvironmentSlot, RlvExtFacts, RlvSession, SETTING_DEBUG,
    SETTING_DEBUG_HIDE_UNSET_DUPLICATE, SETTING_NO_SET_ENV, ViewerRlvExt, object_attachment,
    rlv_flag, rlv_is_enabled, swallows_owner_say,
};
use sl_viewer_world_api::{AvatarControls, ObjectState};
use uuid::Uuid;

use crate::rlv_console::{command_text, is_unset_or_duplicate, outcome_stream, report_line};

// --- Pure command handling ------------------------------------------------

/// What running one owner-say line asked of the viewer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OwnerSayRun {
    /// Whether the held set changed, so the floaters watching it rebuild.
    pub changed: bool,
    /// The heading `@setrot` asked the avatar to face, if the line carried one.
    pub rotate_to: Option<f32>,
    /// The lines to chat back, in the order they were produced.
    pub replies: Vec<RlvReply>,
    /// The per-command report `RestrainedLoveDebug` echoes into the console.
    /// Built whether or not the setting is on, because it is a handful of
    /// strings per owner-say line and building it here keeps the decision to
    /// *show* it in one place.
    pub echo: Vec<(RlvConsoleKind, String)>,
}

/// Run one owner-say line against `state` as `issuer`, collecting the replies
/// it owes and the report the console echoes.
///
/// The dispatch order is the reference's, and the order matters: the state
/// machine sees every command first, hands back anything that is not a
/// restriction, and only then is it offered to the two registered extension
/// handlers — the debug window (`@getdebug_*` / `@setdebug_*` / `@setrot`) and
/// then the environment (`@getenv_*` / `@setenv_*`) — and finally answered as a
/// query. Nothing in the behaviour dictionary can be shadowed by an extension
/// that way.
///
/// # Queries the viewer cannot honestly answer
///
/// A query that reads the avatar, the inventory or the camera
/// ([`RlvQuery::needs_source`]) is **not** answered: this viewer has no
/// [`RlvQuerySource`](sl_rlv::RlvQuerySource) wired to those mirrors yet, and a
/// `@getattach` answered all-zeros for a fully dressed avatar is a lie the
/// script acts on — worse for it than silence. The rest of the language — the
/// `@version` handshake every device opens with, `@getstatus`, `@getcommand`,
/// the `@getcam_*` limits — needs no viewer facts and *is* answered, so a
/// device can tell it is talking to an RLV viewer.
///
/// Split out from the system so the whole decision is testable without an app.
pub fn apply_owner_say(
    state: &mut RlvState,
    issuer: Uuid,
    agent: Uuid,
    line: &str,
    hide_unset_duplicate: bool,
    ext: &mut impl RlvExtSource,
    env: &mut impl RlvEnvSource,
) -> OwnerSayRun {
    let mut run = OwnerSayRun::default();
    let Some(parsed) = parse_chat_line(line) else {
        return run;
    };
    let source = RlvNoFacts::new(agent);
    for command in parsed {
        // A field the grammar rejects: the reference counts it a failure and
        // says so in its debug output. There is no reply to build, because
        // there is no command to have named a channel.
        let command = match command {
            Ok(command) => command,
            Err(error) => {
                run.echo.push((RlvConsoleKind::Error, error.to_string()));
                continue;
            }
        };
        let text = command_text(&command);
        let is_query = matches!(command.param, RlvParam::Reply { .. });
        let applied = state.apply(issuer, &command);
        if applied.succeeded() {
            run.changed = true;
        }
        let mut outcome = applied;
        // The state machine hands back every `=force` action and every query;
        // one of those may still be an extension command, which is the last
        // place a keyword can be recognised.
        if applied == RlvOutcome::NotAStateChange {
            if let Some(result) = state.run_extension(issuer, &command, ext) {
                outcome = result.outcome;
                if let Some(heading) = result.rotate_to {
                    run.rotate_to = Some(heading);
                }
                if let Some(reply) = result.reply {
                    run.replies.push(reply);
                }
            } else if let Some(result) = state.run_environment(issuer, &command, env) {
                // The second registered handler, in the reference's own order:
                // the `@getenv_*` / `@setenv_*` sky. It never moves the avatar
                // and never changes a restriction, so all it can leave behind is
                // an answer.
                outcome = result.outcome;
                if let Some(reply) = result.reply {
                    run.replies.push(reply);
                }
            } else if is_query {
                match RlvQuery::classify(&command) {
                    // See the "Queries the viewer cannot honestly answer"
                    // section: the answer would be a lie, so there is none.
                    Ok(query) if query.needs_source() => {
                        outcome = RlvOutcome::Failed;
                        run.echo.push((
                            RlvConsoleKind::Error,
                            format!("@{text}: not answered — the query source is not wired up yet"),
                        ));
                    }
                    // Both the recognised source-free queries and the ones that
                    // do not classify: a failed query is still answered, with an
                    // empty string, or the script waits forever.
                    _ => {
                        let answer = state.answer(issuer, &command, &source);
                        outcome = answer.outcome;
                        if let Some(reply) = answer.reply {
                            run.replies.push(reply);
                        }
                    }
                }
            }
        }
        if hide_unset_duplicate && is_unset_or_duplicate(outcome) {
            continue;
        }
        run.echo
            .push((outcome_stream(outcome), report_line(&text, outcome)));
    }
    run
}

// --- The master switch ----------------------------------------------------

/// The notification raised when RLV is switched **on** mid-session.
///
/// No reference counterpart: there the setting needs a restart, so the toggle
/// only ever promises a change rather than making one.
pub const NOTIFY_TOGGLED_ON: &str = "RLVaToggledOn";

/// The notification raised when RLV is switched **off** mid-session.
pub const NOTIFY_TOGGLED_OFF: &str = "RLVaToggledOff";

/// What a look at the `RestrainedLove` master switch found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasterSwitch {
    /// The first look of the session: whatever it says, this is the state the
    /// user logged in with and there is nothing to report.
    Initial,
    /// It has not moved since the last look.
    Unchanged,
    /// It moved, to this value, while the session was already running.
    Toggled {
        /// Whether RLV is now on.
        enabled: bool,
    },
}

impl MasterSwitch {
    /// The notification a transition raises, if any.
    #[must_use]
    pub const fn notification(self) -> Option<&'static str> {
        match self {
            Self::Toggled { enabled: true } => Some(NOTIFY_TOGGLED_ON),
            Self::Toggled { enabled: false } => Some(NOTIFY_TOGGLED_OFF),
            Self::Initial | Self::Unchanged => None,
        }
    }
}

/// What a look at any boolean setting found.
///
/// Three answers rather than two, because "it is on" and "it has just been
/// turned on" are different things to a system that has to both *apply* a
/// setting and *report* a change in it: the first look of a session is the
/// state the user logged in with, and telling them it just happened would be a
/// lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagLook {
    /// The first look of the session, with the value found.
    Initial(bool),
    /// It has not moved since the last look.
    Unchanged,
    /// It moved to this value while the session was already running.
    Moved(bool),
}

/// Compare a boolean setting against the last value seen, and record the new
/// one.
///
/// `last` is `None` before the first look.
pub const fn observe_flag(last: &mut Option<bool>, value: bool) -> FlagLook {
    match last.replace(value) {
        None => FlagLook::Initial(value),
        Some(previous) if previous == value => FlagLook::Unchanged,
        Some(_) => FlagLook::Moved(value),
    }
}

/// Compare the switch's current value against the last one seen, and record
/// the new one.
///
/// `last` is `None` before the first look. That case is
/// [`MasterSwitch::Initial`] and is **not** a toggle: a user who has RLV on
/// from the moment they log in must not be told it was just turned on.
///
/// Split out from the system so the one thing worth getting right here — that
/// the first frame is silent and every real move is not — is testable without
/// an app.
pub const fn observe_master_switch(last: &mut Option<bool>, enabled: bool) -> MasterSwitch {
    match observe_flag(last, enabled) {
        FlagLook::Initial(_) => MasterSwitch::Initial,
        FlagLook::Unchanged => MasterSwitch::Unchanged,
        FlagLook::Moved(enabled) => MasterSwitch::Toggled { enabled },
    }
}

// --- Object expiry --------------------------------------------------------

/// How often the restricting objects are checked against the world mirror.
///
/// The reference runs its collector every 30 seconds
/// (`RlvHandler::onGC`), because it is walking the whole object list. This
/// walks the handful of objects that are actually holding restrictions, so it
/// can afford to look far more often — and it has to, because 30 seconds of
/// restraint after the collar came off is 30 seconds the user did not agree to.
pub const OBJECT_WATCH_INTERVAL_SECONDS: f32 = 2.0;

/// How many consecutive checks an object that **was** streamed has to be
/// missing from the world mirror before its restrictions are lifted.
///
/// The reference expires such an object at once. This viewer cannot: its object
/// mirror is **purged wholesale** on a fresh-circuit teleport, so "not in the
/// mirror" is also what every worn attachment looks like for the moment after
/// arriving. [`WORLD_RESET_GRACE_TICKS`] covers the teleport; these strikes
/// cover an ordinary re-stream hiccup, and cost a detached object about six
/// seconds of afterlife.
pub const GONE_STRIKES: u32 = 3;

/// How many consecutive checks an object that was **never** streamed is given
/// before its restrictions are lifted — the reference's own arithmetic
/// (20 misses at one every 30 seconds, "about ten minutes"), expressed against
/// this module's faster tick so the deadline is the same wall-clock time.
pub const UNRESOLVED_STRIKES: u32 = 300;

/// How many checks every watched object is excused after the world mirror was
/// purged (a distant teleport). The mirror is empty through no fault of the
/// objects in it, and they need time to re-stream; a minute is long enough for
/// a slow region and short enough that a collar taken off during the teleport
/// is still released promptly.
pub const WORLD_RESET_GRACE_TICKS: u32 = 30;

/// What is known about one restricting object's presence in the world mirror.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObjectWatch {
    /// Whether this object has ever been found in the world mirror. The
    /// reference's `m_fLookup`: an object that once existed and now does not is
    /// gone, while one that never existed may simply not have streamed yet.
    pub resolved: bool,
    /// Consecutive checks this object has been missing (the reference's
    /// `m_nLookupMisses`).
    pub misses: u32,
    /// Checks still to be excused after a world purge.
    pub grace: u32,
}

impl ObjectWatch {
    /// Whether this object's restrictions should be lifted now.
    #[must_use]
    pub const fn is_expired(&self) -> bool {
        if self.resolved {
            self.misses >= GONE_STRIKES
        } else {
            self.misses >= UNRESOLVED_STRIKES
        }
    }
}

/// The per-object presence bookkeeping the expiry pass keeps, and its tick.
///
/// It lives here rather than on [`RlvState`] because "is this object in the
/// world mirror" is a fact about *this* viewer's scene, not about the RLV
/// language — the pure crate has no object list to ask.
#[derive(Resource, Debug, Default)]
pub struct RlvObjectWatch {
    /// What is known about each restricting object.
    watched: HashMap<Uuid, ObjectWatch>,
    /// Seconds since the last check.
    elapsed: f32,
}

impl RlvObjectWatch {
    /// What is known about `object`, for a caller inspecting the pass.
    #[must_use]
    pub fn get(&self, object: Uuid) -> Option<ObjectWatch> {
        self.watched.get(&object).copied()
    }

    /// Excuse every watched object the next [`WORLD_RESET_GRACE_TICKS`] checks
    /// and forget the misses they have accrued — the world mirror was purged,
    /// so their absence says nothing about them.
    pub fn forgive_world_reset(&mut self) {
        for watch in self.watched.values_mut() {
            watch.misses = 0;
            watch.grace = WORLD_RESET_GRACE_TICKS;
        }
    }

    /// Record that `object` is present in the world mirror.
    pub fn seen(&mut self, object: Uuid) {
        let watch = self.watched.entry(object).or_default();
        watch.resolved = true;
        watch.misses = 0;
    }

    /// Record that `object` is absent, and answer whether that has now gone on
    /// long enough to lift its restrictions.
    pub fn missed(&mut self, object: Uuid) -> bool {
        let watch = self.watched.entry(object).or_default();
        if watch.grace > 0 {
            watch.grace = watch.grace.saturating_sub(1);
            return false;
        }
        watch.misses = watch.misses.saturating_add(1);
        watch.is_expired()
    }

    /// Stop watching `object` — it holds nothing any more.
    pub fn forget(&mut self, object: Uuid) {
        self.watched.remove(&object);
    }

    /// Keep only the objects in `held`. An object that lifted its last
    /// restriction is not being watched for anything, and leaving its entry
    /// behind would carry stale misses into a later restriction from the same
    /// object.
    pub fn retain(&mut self, held: &[Uuid]) {
        self.watched.retain(|object, _watch| held.contains(object));
    }

    /// Advance the tick by `delta` seconds and answer whether a check is due,
    /// consuming the interval when it is.
    pub fn tick(&mut self, delta: f32) -> bool {
        self.elapsed += delta;
        if self.elapsed < OBJECT_WATCH_INTERVAL_SECONDS {
            return false;
        }
        self.elapsed = 0.0;
        true
    }
}

// --- Plugin ---------------------------------------------------------------

/// The RLV command intake: the owner-say gate, the chat-back drain and the
/// object-expiry pass.
///
/// Not part of [`RlvUiPlugins`](crate::RlvUiPlugins): those are the four
/// windows, and this is the wiring that makes the engine reachable at all. A
/// host that wants a viewer an object can restrain adds this; one that only
/// wants to *look* at the state does not have to.
#[derive(Debug, Clone, Copy, Default)]
pub struct RlvIntakePlugin;

impl Plugin for RlvIntakePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RlvObjectWatch>()
            // The seam the `@setenv_*` family writes through. Registered here
            // for the same reason the notification message below is: a host
            // that wants the intake without the whole UI still gets a plugin
            // that runs, and `init_resource` is idempotent, so the UI plugins
            // declaring it too costs nothing.
            .init_resource::<RlvEnvironmentSlot>()
            // Registered here rather than left to the notification host, so a
            // host that wants the intake without the whole UI still gets a
            // plugin that runs. `add_message` is idempotent, so the host
            // registering it too costs nothing.
            .add_message::<ShowNotification>()
            .add_systems(
                Update,
                (
                    // Ahead of the intake, so the frame the switch moves is
                    // reported before anything it changes is acted on.
                    report_master_switch_change,
                    // Likewise ahead of it, so a command arriving in the same
                    // frame the user blocked its keyword is already refused.
                    apply_blocked_behaviours,
                    take_rlv_owner_say,
                    resolve_rlv_objects,
                    // Runs after all three, so a reply an arriving command
                    // produced — and a `@notify` an expiry produced — goes out
                    // in the same frame rather than a frame late.
                    drain_rlv_replies,
                )
                    .chain(),
            );
    }
}

// --- Systems --------------------------------------------------------------

/// Tell the user when the `RestrainedLove` master switch moves mid-session,
/// because in this viewer it moves *now* and the reference's never did.
///
/// The reference requires a restart for the setting to take effect, so its own
/// toggle alert only ever says "…after you restart" and its menu item wears a
/// "(pending restart)" suffix until you do. Applying it at once is friendlier,
/// but it leaves the user in a state the reference cannot reach, and in both
/// directions the state has a consequence they will otherwise meet as a bug:
///
/// - **on** — a device asks whether the viewer speaks RLV when it is attached.
///   Anything worn while the switch was off asked and was told no, and will not
///   ask again: it has to be re-attached before it works, however correctly the
///   viewer now behaves;
/// - **off** — every restriction is released (see
///   [`RlvSession::release_all`]) and the intake stops taking owner-say lines,
///   so the `@`-commands a still-worn device keeps issuing stop being swallowed
///   and start appearing in chat as ordinary text from that object.
///
/// Neither is a fault, and both look exactly like one.
///
/// The release is what makes "off" mean off. The reference reaches the same
/// state by restarting — which is what its switch demands — and a viewer that
/// applied the switch at once *without* releasing would invent a state the
/// reference cannot reach: still restrained by a collar, with the windows that
/// could show it greyed out because RLV is off.
fn report_master_switch_change(
    settings: Option<Res<ViewerSettings>>,
    mut session: ResMut<RlvSession>,
    mut watch: ResMut<RlvObjectWatch>,
    mut notifications: MessageWriter<ShowNotification>,
    mut last: Local<Option<bool>>,
) {
    let Some(settings) = settings else {
        return;
    };
    // Only a settings write can move it, and that is rare; the common frame
    // does not even read the store.
    if !settings.is_changed() && last.is_some() {
        return;
    }
    let enabled = rlv_is_enabled(Some(&settings));
    let change = observe_master_switch(&mut last, enabled);
    let Some(template) = change.notification() else {
        return;
    };
    if !enabled {
        session.release_all();
        // Nothing is being watched for expiry any more, and leaving the stale
        // entries would carry their misses into the next time RLV is on.
        watch.retain(&[]);
    }
    notifications.write(ShowNotification::new(template));
    // The console is the RLV traffic log, and a switch moving under a device
    // is the most consequential thing that can happen to that traffic.
    session.log(
        RlvConsoleKind::Info,
        if enabled {
            "RLV switched on — devices attached while it was off must be re-attached"
        } else {
            "RLV switched off — every restriction released, and owner-say commands              are no longer taken"
        },
    );
}

/// The dictionary row `RestrainedLoveNoSetEnv` takes out of the language.
///
/// `@setenv=n` and `@setenv=y`, and nothing else. The obvious reading of the
/// setting's name is the wrong one: the `@setenv_*` **force** commands are a
/// different keyword of a different kind and keep working, because what the
/// setting refuses is an object *taking the environment away from the user* —
/// the restriction that greys their own environment menu and locks every other
/// object out of the sky — not an object repainting the sky (which the user can
/// undo, and which [`can_change_environment`](sl_viewer_world_api::rlv::can_change_environment)
/// still lets them).
const NO_SET_ENV_ROW: (&str, RlvParamKind) = ("setenv", RlvParamKind::AddRem);

/// Apply `RestrainedLoveNoSetEnv` to the state machine's blocked set, and say so
/// in the console when it moves.
///
/// The reference reads this setting once, while it builds its behaviour
/// dictionary (`rlvhelper.cpp:358`), so there it needs a relog and there can
/// never be a held `@setenv` to reconcile. Here it is live, the same divergence
/// the RLVa strings and the master switch already take — which means the moment
/// the user turns it on, the collar that got in first has to lose the sky.
/// [`RlvState::set_behaviour_blocked`](sl_rlv::RlvState::set_behaviour_blocked)
/// does that releasing; this system is only the bridge from the settings store.
///
/// It is deliberately **not** gated on the master switch. With RLV off nothing
/// is applying commands at all, and keeping the blocked set in step regardless
/// means switching RLV back on does not need a second look at this setting.
fn apply_blocked_behaviours(
    settings: Option<Res<ViewerSettings>>,
    mut session: ResMut<RlvSession>,
    mut last: Local<Option<bool>>,
) {
    let Some(settings) = settings else {
        return;
    };
    // Only a settings write can move it; the common frame reads nothing and,
    // just as importantly, takes no mutable borrow of the session.
    if !settings.is_changed() && last.is_some() {
        return;
    }
    let blocked = rlv_flag(Some(&settings), SETTING_NO_SET_ENV);
    let look = observe_flag(&mut last, blocked);
    if look == FlagLook::Unchanged {
        return;
    }
    // Read before the block, because blocking is what takes it away.
    let held = session.state().has_behaviour(RlvBehaviour::Setenv);
    if !session
        .state_mut()
        .set_behaviour_blocked(NO_SET_ENV_ROW.0, NO_SET_ENV_ROW.1, blocked)
    {
        error!(
            "no `@{}` restriction to block: `{SETTING_NO_SET_ENV}` cannot be honoured",
            NO_SET_ENV_ROW.0
        );
        return;
    }
    // A released restriction is a change the floaters watching the held set
    // have to redraw for; turning the setting *off* releases nothing.
    if blocked && held {
        session.bump();
    }
    if let FlagLook::Moved(_) = look {
        session.log(
            RlvConsoleKind::Info,
            if blocked {
                "@setenv=n is now refused — no object may take your environment away, and any that had it has lost it"
            } else {
                "@setenv=n is accepted again — an object may take your environment away"
            },
        );
    }
}

/// Feed every arriving owner-say `@`-line to the state machine as the object
/// that said it.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the event stream, the \
              identity and settings that decide whether a line is taken at all, the two facts \
              the debug-setting allowlist reads, the world mirror an issuer is resolved \
              against, the movement controls a forced rotation writes, the environment seam \
              the sky family writes through, and the session it all lands in"
)]
fn take_rlv_owner_say(
    mut events: MessageReader<SlEvent>,
    identity: Option<Res<SlIdentity>>,
    settings: Option<Res<ViewerSettings>>,
    facts: Option<Res<RlvExtFacts>>,
    objects: Option<Res<ObjectState>>,
    mut controls: Option<ResMut<AvatarControls>>,
    mut environment: ResMut<RlvEnvironmentSlot>,
    mut session: ResMut<RlvSession>,
) {
    if !rlv_is_enabled(settings.as_deref()) {
        // Drain, so flipping the master switch on does not replay everything an
        // object said while it was off.
        events.clear();
        return;
    }
    let agent = identity
        .as_deref()
        .and_then(|identity| identity.agent_id)
        .map_or_else(Uuid::nil, |agent| agent.uuid());
    let hide = rlv_flag(settings.as_deref(), SETTING_DEBUG_HIDE_UNSET_DUPLICATE);
    let debug = rlv_flag(settings.as_deref(), SETTING_DEBUG);
    for event in events.read() {
        let SlSessionEvent::ChatReceived(message) = &event.0 else {
            continue;
        };
        if !swallows_owner_say(
            settings.as_deref(),
            objects.as_deref(),
            message.source,
            message.chat_type,
            &message.message,
        ) {
            continue;
        }
        // `CHAT_TYPE_OWNER` is what an object says; anything else wearing that
        // type is not something a restriction may be attributed to, and an
        // unattributable restriction is one nothing can ever lift.
        let ChatSource::Object(issuer) = message.source else {
            continue;
        };
        let issuer = issuer.uuid();
        // Where the object sits on the avatar, cached the first time it is
        // known: a bare `@detach=n` locks *that* attachment, and `@detach=y`
        // may well arrive after the object is gone.
        if let Some(objects) = objects.as_deref()
            && session.state().object_attachment(issuer).is_none()
            && let Some(attachment) = object_attachment(objects, ObjectKey::from(issuer))
        {
            session
                .state_mut()
                .set_object_attachment(issuer, Some(attachment));
        }
        let mut ext = ViewerRlvExt {
            settings: settings.as_deref(),
            facts: facts.as_deref().copied().unwrap_or_default(),
        };
        let run = apply_owner_say(
            session.state_mut(),
            issuer,
            agent,
            &message.message,
            hide,
            &mut ext,
            &mut *environment,
        );
        if run.changed {
            session.bump();
        }
        for reply in run.replies {
            session.push_reply(reply);
        }
        // `RestrainedLoveDebug` exists for exactly this: an echo of every
        // command an object processed. Until now nothing filled it, because
        // only the console processed commands and it echoes regardless.
        if debug {
            for (kind, text) in run.echo {
                session.log(kind, text);
            }
        }
        // `@setrot` is the one extension command that moves something: the
        // movement driver picks the heading up on its next frame, exactly as it
        // does for the same command typed into the console.
        if let (Some(heading), Some(controls)) = (run.rotate_to, controls.as_mut()) {
            controls.forced_heading = Some(heading);
        }
    }
}

/// Walk every restricting object against the world mirror, caching where it is
/// worn the first time it resolves and lifting what it holds once it is gone.
fn resolve_rlv_objects(
    time: Res<Time>,
    identity: Option<Res<SlIdentity>>,
    objects: Option<Res<ObjectState>>,
    mut events: MessageReader<SlEvent>,
    mut session: ResMut<RlvSession>,
    mut watch: ResMut<RlvObjectWatch>,
) {
    // A distant teleport empties the world mirror with no per-object removal,
    // so every restricting object would look derezzed at once.
    // `count`, not `any`: `any` would stop reading at the first match and leave
    // the rest of the frame's events for the next one, where the same reset
    // would forgive a second time.
    let resets = events
        .read()
        .filter(|event| {
            matches!(
                event.0,
                SlSessionEvent::RegionChanged {
                    world_reset: true,
                    ..
                }
            )
        })
        .count();
    if resets > 0 {
        watch.forgive_world_reset();
    }
    if !watch.tick(time.delta_secs()) {
        return;
    }
    let Some(objects) = objects.as_deref() else {
        return;
    };
    // The agent is an issuer too — that is how the RLVa console works — and it
    // is not an object in the world mirror, so it must never be collected. The
    // console lifts its own restrictions when it closes.
    let agent = identity
        .as_deref()
        .and_then(|identity| identity.agent_id)
        .map_or_else(Uuid::nil, |agent| agent.uuid());
    let restricting: Vec<Uuid> = session
        .state()
        .restricting_objects()
        .filter(|object| *object != agent)
        .collect();
    let mut expired = Vec::new();
    for object in restricting {
        match object_attachment(objects, ObjectKey::from(object)) {
            Some(attachment) => {
                watch.seen(object);
                if session.state().object_attachment(object) != Some(attachment) {
                    session
                        .state_mut()
                        .set_object_attachment(object, Some(attachment));
                }
            }
            // Not worn is not missing: an in-world prim the agent owns is a
            // legitimate issuer, so presence is asked of the mirror itself.
            None if objects.entity_of(ObjectKey::from(object)).is_some() => watch.seen(object),
            None => {
                if watch.missed(object) {
                    expired.push(object);
                }
            }
        }
    }
    for object in expired {
        info!("rlv: garbage collecting restrictions from vanished object {object}");
        session.state_mut().clear_object(object);
        watch.forget(object);
        session.bump();
    }
    // Anything that stopped restricting is no longer worth watching.
    let held: Vec<Uuid> = session.state().restricting_objects().collect();
    watch.retain(&held);
}

/// Shout every queued reply on the channel it names.
///
/// The reference *shouts* (`RlvUtil::sendChatReply` sends `CHAT_TYPE_SHOUT`) so
/// the answer carries the full 100 m, and truncates rather than splitting — the
/// truncation has already happened in `sl-rlv`, which built the line.
fn drain_rlv_replies(mut session: ResMut<RlvSession>, mut commands: MessageWriter<SlCommand>) {
    if session.state().has_notifications() {
        let owed = session.state_mut().take_notifications();
        for notification in owed {
            session.push_reply(RlvReply::from(notification));
        }
    }
    if !session.has_replies() {
        return;
    }
    for reply in session.take_replies() {
        // Channel `0` is local chat, where everybody nearby would read it; the
        // engine only ever means the debug console by it. Nothing that reaches
        // this queue should carry one, so say so rather than shouting it.
        if !is_valid_reply_channel(reply.channel, false) {
            warn!(
                "rlv: dropping a reply for channel {} — not a channel a reply may go on",
                reply.channel
            );
            continue;
        }
        commands.write(SlCommand(Command::Chat {
            message: reply.message,
            chat_type: ChatType::Shout,
            channel: ChatChannel(reply.channel),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FlagLook, GONE_STRIKES, MasterSwitch, NOTIFY_TOGGLED_OFF, NOTIFY_TOGGLED_ON,
        OBJECT_WATCH_INTERVAL_SECONDS, OwnerSayRun, RlvObjectWatch, UNRESOLVED_STRIKES,
        WORLD_RESET_GRACE_TICKS, apply_owner_say, observe_flag, observe_master_switch,
    };
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_rlv::{RlvBehaviour, RlvState, is_valid_reply_channel};
    use sl_settings::{Scope, SettingValue, SettingsStore};
    use sl_viewer_settings::ViewerSettings;
    use sl_viewer_world_api::rlv::{
        RlvConsoleKind, RlvEnvironmentSlot, RlvExtFacts, RlvSession, SETTING_NO_SET_ENV,
        ViewerRlvExt, register_settings,
    };
    use uuid::Uuid;

    /// A `Box<dyn Error>` alias, so a test can use `?`.
    type TestError = Box<dyn core::error::Error>;

    /// The agent this viewer is logged in as.
    const AGENT: Uuid = Uuid::from_u128(0x0a);

    /// The collar saying things.
    const COLLAR: Uuid = Uuid::from_u128(1);

    /// Run one owner-say line against `state` as the collar.
    fn say(state: &mut RlvState, line: &str) -> OwnerSayRun {
        let mut ext = ViewerRlvExt {
            settings: None,
            facts: RlvExtFacts::default(),
        };
        let mut env = RlvEnvironmentSlot::default();
        apply_owner_say(state, COLLAR, AGENT, line, false, &mut ext, &mut env)
    }

    /// The whole point: a line an object says restrains the viewer, attributed
    /// to that object so taking it off lifts exactly what it held.
    #[test]
    fn an_object_line_restrains_the_viewer() {
        let mut state = RlvState::new();
        let run = say(&mut state, "@detach=n,fly=n");
        assert!(run.changed);
        assert!(state.has_behaviour(RlvBehaviour::Fly));
        // A bare `@detach=n` locks *this* object on rather than counting
        // against a global behaviour, so it is asked for by its issuer.
        assert!(state.has_behaviour_from(COLLAR, RlvBehaviour::Detach, ""));
        state.clear_object(COLLAR);
        assert!(!state.has_behaviour(RlvBehaviour::Fly));
        assert!(!state.has_behaviour_from(COLLAR, RlvBehaviour::Detach, ""));
    }

    /// The handshake every RLV device opens with is answered, on the channel it
    /// named and shoutable — without it a collar concludes this is not an RLV
    /// viewer and gives up before it has started.
    #[test]
    fn the_version_handshake_is_answered() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let run = say(&mut state, "@version=2222");
        let reply = run.replies.first().ok_or("no reply")?;
        assert_eq!(reply.channel, 2222);
        assert!(reply.message.contains("RestrainedLife viewer"));
        assert!(is_valid_reply_channel(reply.channel, false));
        Ok(())
    }

    /// `@getstatus` reads only the state machine, so it is answered in full —
    /// with the restrictions this very line put in place.
    #[test]
    fn getstatus_is_answered_from_the_state_machine() -> Result<(), TestError> {
        let mut state = RlvState::new();
        say(&mut state, "@fly=n");
        let run = say(&mut state, "@getstatus=2222");
        assert_eq!(run.replies.first().ok_or("no reply")?.message, "/fly");
        Ok(())
    }

    /// A query that reads the avatar is **not** answered with a made-up answer:
    /// `@getattach` replying all-zeros for a dressed avatar is a lie the script
    /// would act on. It says so on the console instead.
    #[test]
    fn a_query_that_needs_viewer_facts_is_not_answered() {
        let mut state = RlvState::new();
        let run = say(&mut state, "@getattach=2222");
        assert!(run.replies.is_empty());
        assert!(
            run.echo
                .iter()
                .any(|(_kind, text)| text.contains("not answered")),
            "the console should say why: {:?}",
            run.echo
        );
    }

    /// An extension read is answered through the same seam as a query, so the
    /// consumer has one queue rather than two.
    #[test]
    fn an_extension_read_answers_on_the_same_seam() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let run = say(&mut state, "@getdebug_restrainedlovenosetenv=2222");
        let reply = run.replies.first().ok_or("no reply")?;
        assert_eq!(reply.channel, 2222);
        assert_eq!(reply.message, "0");
        Ok(())
    }

    /// A `@notify` subscriber hears about the next command, and what it is owed
    /// is a reply like any other — same channel rules, same send.
    #[test]
    fn a_notify_subscription_is_owed_a_reply() -> Result<(), TestError> {
        let mut state = RlvState::new();
        say(&mut state, "@notify:2222=n");
        say(&mut state, "@sendchat=n");
        let owed = state.take_notifications();
        let notification = owed.last().ok_or("nothing owed")?.clone();
        assert_eq!(notification.channel, 2222);
        assert_eq!(notification.message, "/sendchat=n");
        let reply = sl_rlv::RlvReply::from(notification);
        assert_eq!(reply.channel, 2222);
        assert_eq!(reply.message, "/sendchat=n");
        Ok(())
    }

    /// Every command on the line is reported, one echo line each, and a
    /// malformed field does not sink its neighbours.
    #[test]
    fn every_command_is_echoed_once() {
        let mut state = RlvState::new();
        let run = say(&mut state, "@fly=n,nonsense,detach=n");
        assert_eq!(run.echo.len(), 3);
        assert!(state.has_behaviour(RlvBehaviour::Fly));
        assert!(state.has_behaviour_from(COLLAR, RlvBehaviour::Detach, ""));
    }

    /// `RLVaDebugHideUnsetDuplicate` drops the echo of a command that changed
    /// nothing, and only that echo — the command still applied.
    #[test]
    fn hiding_unset_duplicates_drops_only_those() {
        let mut state = RlvState::new();
        let mut ext = ViewerRlvExt {
            settings: None,
            facts: RlvExtFacts::default(),
        };
        let mut env = RlvEnvironmentSlot::default();
        apply_owner_say(
            &mut state, COLLAR, AGENT, "@fly=n", true, &mut ext, &mut env,
        );
        let run = apply_owner_say(
            &mut state,
            COLLAR,
            AGENT,
            "@fly=n,detach=n",
            true,
            &mut ext,
            &mut env,
        );
        assert_eq!(run.echo.len(), 1);
        assert!(
            run.echo
                .first()
                .is_some_and(|(kind, _text)| *kind == RlvConsoleKind::Info)
        );
    }

    /// A line that is not an RLV line at all does nothing — the gate should
    /// never hand one over, and if it did there is nothing to run.
    #[test]
    fn a_line_without_the_prefix_does_nothing() {
        let mut state = RlvState::new();
        let run = say(&mut state, "hello there");
        assert_eq!(run, OwnerSayRun::default());
    }

    /// An object that was streamed and then vanished is collected after
    /// [`GONE_STRIKES`] checks — and not before, so a one-frame gap in the
    /// world mirror cannot strip a collar.
    #[test]
    fn a_vanished_object_expires_after_its_strikes() {
        let mut watch = RlvObjectWatch::default();
        watch.seen(COLLAR);
        for _strike in 1..GONE_STRIKES {
            assert!(!watch.missed(COLLAR));
        }
        assert!(watch.missed(COLLAR));
    }

    /// An object that was **never** streamed gets the reference's full ten
    /// minutes instead: it may simply not have arrived yet.
    #[test]
    fn an_unresolved_object_gets_the_long_deadline() {
        let mut watch = RlvObjectWatch::default();
        for _strike in 1..UNRESOLVED_STRIKES {
            assert!(!watch.missed(COLLAR));
        }
        assert!(watch.missed(COLLAR));
    }

    /// A distant teleport purges the world mirror wholesale, so every object in
    /// it goes missing at once through no fault of its own. The grace is what
    /// stops that from stripping every restriction the agent is under.
    #[test]
    fn a_world_reset_does_not_strip_the_agent() {
        let mut watch = RlvObjectWatch::default();
        watch.seen(COLLAR);
        watch.forgive_world_reset();
        for _tick in 0..WORLD_RESET_GRACE_TICKS {
            assert!(!watch.missed(COLLAR));
        }
        // The grace is spent, and the ordinary strikes resume from zero.
        for _strike in 1..GONE_STRIKES {
            assert!(!watch.missed(COLLAR));
        }
        assert!(watch.missed(COLLAR));
    }

    /// Being seen again clears the misses, so an object that flickers out of
    /// the mirror and back is not collected on its next absence.
    #[test]
    fn being_seen_again_clears_the_misses() {
        let mut watch = RlvObjectWatch::default();
        watch.seen(COLLAR);
        assert!(!watch.missed(COLLAR));
        watch.seen(COLLAR);
        assert_eq!(watch.get(COLLAR).map(|entry| entry.misses), Some(0));
    }

    /// An object that lifted its last restriction is dropped from the watch, so
    /// its stale misses cannot be inherited by a later restriction from it.
    #[test]
    fn the_watch_keeps_only_what_still_restricts() {
        let mut watch = RlvObjectWatch::default();
        watch.seen(COLLAR);
        watch.retain(&[]);
        assert_eq!(watch.get(COLLAR), None);
    }

    /// The tick fires on the interval and not before, so the pass runs at the
    /// documented cadence whatever the frame rate is.
    #[test]
    fn the_pass_runs_on_its_own_interval() {
        let mut watch = RlvObjectWatch::default();
        let frame = OBJECT_WATCH_INTERVAL_SECONDS / 4.0;
        assert!(!watch.tick(frame));
        assert!(!watch.tick(frame));
        assert!(!watch.tick(frame));
        assert!(watch.tick(frame));
        assert!(!watch.tick(frame));
    }

    /// The first look at the master switch is never a toggle, whichever way it
    /// reads. A user who logs in with RLV already on must not be told it was
    /// just turned on — and one who logs in with it off must not be told it was
    /// just turned off, which would be worse: it reads as something having
    /// interfered with their viewer.
    #[test]
    fn the_first_look_at_the_switch_is_never_a_toggle() {
        for start in [true, false] {
            let mut last = None;
            assert_eq!(
                observe_master_switch(&mut last, start),
                MasterSwitch::Initial
            );
            assert_eq!(observe_master_switch(&mut last, start).notification(), None);
        }
    }

    /// Every real move is reported, in the direction it moved, and a re-read
    /// that found nothing new is not.
    #[test]
    fn each_move_of_the_switch_is_reported_once() {
        let mut last = None;
        assert_eq!(
            observe_master_switch(&mut last, false),
            MasterSwitch::Initial
        );
        assert_eq!(
            observe_master_switch(&mut last, true),
            MasterSwitch::Toggled { enabled: true }
        );
        assert_eq!(
            observe_master_switch(&mut last, true),
            MasterSwitch::Unchanged
        );
        assert_eq!(
            observe_master_switch(&mut last, false),
            MasterSwitch::Toggled { enabled: false }
        );
    }

    /// Each direction raises its own notification, and neither the initial look
    /// nor a settled switch raises anything.
    #[test]
    fn the_two_directions_raise_their_own_notifications() {
        assert_eq!(
            MasterSwitch::Toggled { enabled: true }.notification(),
            Some(NOTIFY_TOGGLED_ON)
        );
        assert_eq!(
            MasterSwitch::Toggled { enabled: false }.notification(),
            Some(NOTIFY_TOGGLED_OFF)
        );
        assert_eq!(MasterSwitch::Initial.notification(), None);
        assert_eq!(MasterSwitch::Unchanged.notification(), None);
    }

    /// Both names are in the notification catalogue, so the raise resolves to a
    /// template rather than being dropped with a log nobody reads.
    #[test]
    fn both_toggle_notifications_are_catalogued() {
        for name in [NOTIFY_TOGGLED_ON, NOTIFY_TOGGLED_OFF] {
            assert!(
                sl_viewer_notifications::template(name).is_some(),
                "{name} is not in the catalogue"
            );
        }
    }

    // --- `RestrainedLoveNoSetEnv` ------------------------------------------

    /// The same three-way look the master switch takes, on the setting every
    /// other boolean one is read with.
    #[test]
    fn a_flag_reports_its_first_look_apart_from_every_move() {
        let mut last = None;
        assert_eq!(observe_flag(&mut last, true), FlagLook::Initial(true));
        assert_eq!(observe_flag(&mut last, true), FlagLook::Unchanged);
        assert_eq!(observe_flag(&mut last, false), FlagLook::Moved(false));
        assert_eq!(observe_flag(&mut last, true), FlagLook::Moved(true));
    }

    /// An app with the bridge system and nothing else — no grid, no login, and
    /// no other RLV system, so what the assertions see is this system's doing.
    fn blocked_app() -> App {
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        let mut app = App::new();
        app.insert_resource(settings)
            .init_resource::<RlvSession>()
            .add_systems(Update, super::apply_blocked_behaviours);
        app
    }

    /// Move the setting the way its RLVa menu entry and the debug-settings
    /// editor do, and run the frame that notices.
    fn set_no_set_env(app: &mut App, enabled: bool) {
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_NO_SET_ENV,
            SettingValue::Bool(enabled),
        );
        app.update();
    }

    /// Say one command as the collar, straight at the session's state machine.
    fn collar_says(app: &mut App, line: &str) -> OwnerSayRun {
        let mut ext = ViewerRlvExt {
            settings: None,
            facts: RlvExtFacts::default(),
        };
        let mut env = RlvEnvironmentSlot::default();
        let mut session = app.world_mut().resource_mut::<RlvSession>();
        apply_owner_say(
            session.state_mut(),
            COLLAR,
            AGENT,
            line,
            false,
            &mut ext,
            &mut env,
        )
    }

    /// The setting the user logged in with is applied — and applied silently,
    /// because nothing moved.
    #[test]
    fn the_setting_is_applied_on_the_first_frame() {
        let mut app = blocked_app();
        set_no_set_env(&mut app, true);
        let run = collar_says(&mut app, "@setenv=n");
        assert!(!run.changed);
        assert!(
            !app.world()
                .resource::<RlvSession>()
                .state()
                .has_behaviour(RlvBehaviour::Setenv)
        );

        // A second app, left at the declared default, takes the command.
        let mut open = blocked_app();
        open.update();
        let run = collar_says(&mut open, "@setenv=n");
        assert!(run.changed);
        assert!(
            open.world()
                .resource::<RlvSession>()
                .state()
                .has_behaviour(RlvBehaviour::Setenv)
        );
    }

    /// Turning it on mid-session takes the sky back off the collar that already
    /// had it, wakes the floaters watching the held set, and says so.
    #[test]
    fn turning_it_on_releases_what_a_collar_already_held() {
        let mut app = blocked_app();
        app.update();
        assert!(collar_says(&mut app, "@setenv=n").changed);
        let before = app.world().resource::<RlvSession>().revision();

        set_no_set_env(&mut app, true);
        let session = app.world().resource::<RlvSession>();
        assert!(!session.state().has_behaviour(RlvBehaviour::Setenv));
        pretty_assertions::assert_ne!(session.revision(), before, "the floaters must redraw");
        assert!(
            session
                .console()
                .iter()
                .any(|line| line.kind == RlvConsoleKind::Info && line.text.contains("@setenv=n")),
            "a move of the setting belongs in the RLV traffic log: {:?}",
            session.console()
        );

        // And turning it off gives the keyword back.
        set_no_set_env(&mut app, false);
        assert!(collar_says(&mut app, "@setenv=n").changed);
        assert!(
            app.world()
                .resource::<RlvSession>()
                .state()
                .has_behaviour(RlvBehaviour::Setenv)
        );
    }

    /// Switching RLV off resets the state machine; the user's own setting is
    /// not the state machine's to reset.
    #[test]
    fn the_blocked_set_survives_the_master_switch() {
        let mut app = blocked_app();
        set_no_set_env(&mut app, true);
        app.world_mut().resource_mut::<RlvSession>().release_all();
        // No settings write, so the bridge does not run again — the carry-over
        // is what has to hold.
        app.update();
        let run = collar_says(&mut app, "@setenv=n");
        assert!(!run.changed);
        assert!(
            !app.world()
                .resource::<RlvSession>()
                .state()
                .has_behaviour(RlvBehaviour::Setenv)
        );
    }
}
