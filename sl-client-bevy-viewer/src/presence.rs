//! **Presence modes** (`viewer-do-not-disturb-away`): Away / auto-AFK, Do Not
//! Disturb (the reference calls the visible state *Unavailable*), and the two
//! Firestorm autorespond modes — plus the canned IM replies they send.
//!
//! # The four modes
//!
//! - **Away** — session state, set by hand from Comm ▸ Online Status or
//!   automatically after [`SETTING_AFK_TIMEOUT`] seconds without input, and
//!   cleared by the next input once it has held [`MIN_AFK_SECS`] (so a stray
//!   mouse twitch while the screen-saver runs does not un-away you). Going away
//!   starts `ANIM_AGENT_AWAY` and raises [`ControlFlags::AWAY`] in the
//!   `AgentUpdate` the movement driver sends.
//! - **Do Not Disturb** — session state, a manual mode that starts
//!   `ANIM_AGENT_DO_NOT_DISTURB` (so every other viewer's tag reads
//!   *Unavailable*), **queues** incoming toasts and offer cards instead of
//!   showing them, and answers IMs with the busy reply.
//! - **Autorespond** and **Autorespond to non-friends** — the Firestorm
//!   extension: answer IMs but keep receiving them normally. Unlike the first
//!   two these **persist per account** ([`SETTING_AUTORESPOND_MODE`] /
//!   [`SETTING_AUTORESPOND_NON_FRIENDS_MODE`]), exactly as the reference stores
//!   them, so they survive a relog.
//!
//! The two session modes deliberately do **not** persist: the reference resets
//! both at login, and a viewer that came back Unavailable after a crash would
//! silently swallow everyone's IMs.
//!
//! # Who sees what
//!
//! Away and Do Not Disturb are broadcast the only way the protocol carries
//! them: as **signalled animations**. The simulator relays our `AgentAnimation`
//! to everyone nearby, whose viewer reads the id out of the signalled set and
//! writes the status line on our tag — which is also how *we* read *their*
//! state ([`crate::name_tag_content`]). There is deliberately no local
//! playback here (unlike [`crate::typing`]): a presence change is not a
//! latency-sensitive gesture, and the simulator's echo of our own
//! `AgentAnimation` is what the own tag reads, so playing it locally as well
//! would only risk double-driving the pose. The autorespond modes have no wire
//! representation at all — they are ours alone, shown only on our own tag.
//!
//! # The replies
//!
//! An IM that arrives while a mode is on gets one canned answer, sent as an
//! [`ImDialog::DoNotDisturbAutoResponse`](sl_client_bevy::ImDialog) IM
//! ([`Command::AutoResponse`]) so the sender's viewer knows it was automatic
//! and never answers it in turn. Following the reference, the reply is sent
//! **once per conversation** — only when no conversation with that resident is
//! open yet — so a back-and-forth is not answered line by line; closing the
//! conversation arms it again. The precedence between modes mirrors
//! `LLIMProcessing::getAutoresponseTextForAvatar`: Do Not Disturb, then
//! autorespond-to-non-friends (for a non-friend), then autorespond, then away
//! (only with [`SETTING_SEND_AWAY_RESPONSE`] on).
//!
//! The **text** of that reply is not always the global one: a
//! [contact set](crate::contact_sets) the sender is filed under may carry its
//! own ([`crate::contact_sets::SetAutoresponseMode`]), and it wins — the
//! reference's `getAutoresponseForFriend`, consulted before
//! `gSavedPerAccountSettings`, which is what makes "my partner gets a different
//! Unavailable message" work. Only the three *mode* replies can be overridden
//! that way; the away and blocked replies are statements about the user, not
//! about the sender, and the reference gives them no per-set layer either.
//!
//! A **blocked** sender is handled first and separately: with
//! [`SETTING_SEND_MUTED_RESPONSE`] on they get the "you are blocked" reply and
//! nothing else, and with it off they get no reply at all, whatever mode is on.
//!
//! Reference (Firestorm, read-only): `llagent.cpp`
//! (`setAFK` / `clearAFK` / `setDoNotDisturb` / `selectAutorespond*`),
//! `llappviewer.cpp` (`idle_afk_check`, the `QuitAfterSecondsOfAFK` check),
//! `llimprocessing.cpp` (`getAutoresponseTextForAvatar` and the reply
//! decision), `menu_viewer.xml` (Comm ▸ Online Status).

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use sl_client_bevy::{
    AnimationKey, Command, ImDialog, SlAgentParcel, SlCommand, SlEvent, SlIdentity, SlSessionEvent,
};
use sl_settings::SettingValue;

use crate::contact_sets::{ContactSets, SetAutoresponseMode};
use crate::conversations::{ConversationKey, ConversationModel, ConversationNotice};
use crate::notifications::ShowNotification;
use crate::settings::ViewerSettings;

/// The settings section the presence modes and their replies live in.
pub(crate) const PRESENCE_SECTION: &[&str] = &["presence"];

/// Whether the autorespond mode is on (the reference `FSAutorespondMode`).
/// Account-scoped and **persisted**, like the reference's per-account flag, so
/// the mode survives a relog.
pub(crate) const SETTING_AUTORESPOND_MODE: &str = "AutorespondMode";

/// Whether the autorespond-to-non-friends mode is on (the reference
/// `FSAutorespondNonFriendsMode`). Account-scoped and persisted.
pub(crate) const SETTING_AUTORESPOND_NON_FRIENDS_MODE: &str = "AutorespondNonFriendsMode";

/// Whether an IM received while merely **away** is answered at all (the
/// reference `FSSendAwayAvatarResponse`; default off — being away is not being
/// busy).
pub(crate) const SETTING_SEND_AWAY_RESPONSE: &str = "SendAwayAvatarResponse";

/// The reply sent to an IM while away, when [`SETTING_SEND_AWAY_RESPONSE`] is
/// on (the reference `FSAwayAvatarResponse`).
pub(crate) const SETTING_AWAY_RESPONSE: &str = "AwayAvatarResponse";

/// Whether a **blocked** resident's IM is answered with
/// [`SETTING_MUTED_RESPONSE`] (the reference `FSSendMutedAvatarResponse`;
/// default off — telling someone they are blocked is a deliberate choice).
pub(crate) const SETTING_SEND_MUTED_RESPONSE: &str = "SendMutedAvatarResponse";

/// The reply sent to a blocked resident's IM, when
/// [`SETTING_SEND_MUTED_RESPONSE`] is on (the reference
/// `FSMutedAvatarResponse`).
pub(crate) const SETTING_MUTED_RESPONSE: &str = "MutedAvatarResponse";

/// Whether going away sits the avatar down on the ground, standing it back up
/// on return (the reference `AvatarSitOnAway`, an anti-grief habit). Default
/// off.
pub(crate) const SETTING_SIT_ON_AWAY: &str = "AvatarSitOnAway";

/// Seconds of *being away* after which the viewer logs out by itself; `0` =
/// never (the reference `QuitAfterSecondsOfAFK`). Distinct from
/// [`SETTING_AFK_TIMEOUT`], which is the idle time before going away.
///
/// [`SETTING_AFK_TIMEOUT`]: crate::preferences_general::SETTING_AFK_TIMEOUT
pub(crate) const SETTING_QUIT_AFTER_AFK: &str = "QuitAfterSecondsOfAFK";

/// The default away reply (the reference `AwayAvatarResponseDefault`).
const AWAY_RESPONSE_DEFAULT: &str = "The Resident you messaged is currently away from keyboard. \
                                     Your message will still be shown in their IM panel for \
                                     later viewing.";

/// The default blocked-sender reply (the reference `MutedAvatarsResponseDefault`).
const MUTED_RESPONSE_DEFAULT: &str =
    "The Resident you messaged has blocked you from sending them any messages.";

/// How long the away state must have held before input clears it (the
/// reference's `LLAgent::MIN_AFK_TIME`) — without it, the mouse move that
/// happens to arrive one frame after the auto-AFK fires would cancel it.
const MIN_AFK_SECS: f32 = 10.0;

/// The short name of the built-in away animation in the [`sl_anim`] registry.
const AWAY_ANIMATION: &str = "away";

/// The short name of the built-in do-not-disturb animation in the [`sl_anim`]
/// registry.
const DND_ANIMATION: &str = "do_not_disturb";

/// Register the presence settings. The two mode flags bind at account scope
/// (the reference keeps them per account); the replies live with the other
/// canned replies on the chat tab; the two away behaviours are global.
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_AUTORESPOND_MODE,
        SettingValue::Bool(false),
        "Answer incoming IMs with the autorespond reply",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_AUTORESPOND_NON_FRIENDS_MODE,
        SettingValue::Bool(false),
        "Answer incoming IMs from non-friends with the autorespond reply",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_SEND_AWAY_RESPONSE,
        SettingValue::Bool(false),
        "Answer incoming IMs while away with the away reply",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_AWAY_RESPONSE,
        SettingValue::String(AWAY_RESPONSE_DEFAULT.to_owned()),
        "The automatic reply sent to IMs while away",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_SEND_MUTED_RESPONSE,
        SettingValue::Bool(false),
        "Tell a blocked resident their IM was not delivered",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_MUTED_RESPONSE,
        SettingValue::String(MUTED_RESPONSE_DEFAULT.to_owned()),
        "The automatic reply sent to a blocked resident's IM",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_SIT_ON_AWAY,
        SettingValue::Bool(false),
        "Sit the avatar down while away, standing back up on return",
    );
    settings.register_in(
        PRESENCE_SECTION,
        SETTING_QUIT_AFTER_AFK,
        SettingValue::U32(0),
        "Seconds of being away before the viewer logs out (0 = never)",
    );
}

/// Which presence mode a canned reply came from — what
/// [`reply_for`] resolved, so the caller can pick the right text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplyMode {
    /// The sender is blocked and [`SETTING_SEND_MUTED_RESPONSE`] is on.
    Muted,
    /// Do Not Disturb is on.
    DoNotDisturb,
    /// Autorespond-to-non-friends is on and the sender is not a friend.
    AutorespondNonFriends,
    /// Autorespond is on.
    Autorespond,
    /// Away, with [`SETTING_SEND_AWAY_RESPONSE`] on.
    Away,
}

impl ReplyMode {
    /// Which per-set override, if any, answers this mode. The away and blocked
    /// replies have none — see the module doc.
    #[must_use]
    const fn set_override(self) -> Option<SetAutoresponseMode> {
        match self {
            Self::DoNotDisturb => Some(SetAutoresponseMode::Busy),
            Self::Autorespond => Some(SetAutoresponseMode::Autorespond),
            Self::AutorespondNonFriends => Some(SetAutoresponseMode::NonFriends),
            Self::Muted | Self::Away => None,
        }
    }
}

/// The mode flags a reply decision reads — pulled out of the settings store and
/// the live presence state so the decision itself is a pure function.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a direct mirror of the independent mode flags the reference's \
              `getAutoresponseTextForAvatar` reads; folding them into enums would \
              invent states the settings cannot express"
)]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ReplyModes {
    /// Away is on.
    pub(crate) away: bool,
    /// Away replies are enabled ([`SETTING_SEND_AWAY_RESPONSE`]).
    pub(crate) send_away: bool,
    /// Do Not Disturb is on.
    pub(crate) do_not_disturb: bool,
    /// Autorespond is on.
    pub(crate) autorespond: bool,
    /// Autorespond-to-non-friends is on.
    pub(crate) autorespond_non_friends: bool,
    /// Blocked-sender replies are enabled ([`SETTING_SEND_MUTED_RESPONSE`]).
    pub(crate) send_muted: bool,
}

/// Decide which canned reply (if any) answers an IM from a sender who is / is
/// not a friend and is / is not blocked — the reference's
/// `getAutoresponseTextForAvatar` precedence, with the blocked case lifted in
/// front of it (the reference decides that one a level up, before the modes are
/// consulted at all).
#[must_use]
pub(crate) const fn reply_for(
    modes: ReplyModes,
    is_friend: bool,
    is_blocked: bool,
) -> Option<ReplyMode> {
    if is_blocked {
        // A blocked sender never reaches the mode chain: either they are told
        // they are blocked, or they hear nothing at all.
        return if modes.send_muted {
            Some(ReplyMode::Muted)
        } else {
            None
        };
    }
    if modes.do_not_disturb {
        return Some(ReplyMode::DoNotDisturb);
    }
    if modes.autorespond_non_friends && !is_friend {
        return Some(ReplyMode::AutorespondNonFriends);
    }
    if modes.autorespond {
        return Some(ReplyMode::Autorespond);
    }
    if modes.away && modes.send_away {
        return Some(ReplyMode::Away);
    }
    None
}

/// The live presence state: the two session modes and the timers behind
/// auto-AFK. The two autorespond modes are **not** here — they are their own
/// persisted settings, read straight from the store wherever they are needed.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the two modes are independent (either, both or neither can be on) and the three \
              remaining flags are per-mode bookkeeping — an enum would have to enumerate every \
              combination to say the same thing"
)]
#[derive(Resource, Debug, Default)]
pub(crate) struct PresenceState {
    /// Whether the avatar is away.
    away: bool,
    /// Whether Do Not Disturb is on.
    do_not_disturb: bool,
    /// Seconds since the last user input (the reference `gAwayTriggerTimer`).
    idle_secs: f32,
    /// Seconds the away state has held (the reference `gAwayTimer`), used for
    /// the clear debounce and the quit-after-AFK timeout.
    away_secs: f32,
    /// The away state last advertised to the simulator, so the animation
    /// request is sent on the edge only.
    advertised_away: bool,
    /// The Do Not Disturb state last advertised, likewise.
    advertised_dnd: bool,
    /// Whether *we* sat the avatar down on going away, so returning only stands
    /// it back up when it was our doing.
    sat_on_away: bool,
}

impl PresenceState {
    /// Whether the avatar is away.
    #[must_use]
    pub(crate) const fn is_away(&self) -> bool {
        self.away
    }

    /// Whether Do Not Disturb is on.
    #[must_use]
    pub(crate) const fn is_do_not_disturb(&self) -> bool {
        self.do_not_disturb
    }

    /// Set the away state, restarting the away clock on a rising edge. The wire
    /// writes are reconciled by [`advertise_presence`].
    pub(crate) const fn set_away(&mut self, away: bool) {
        if self.away != away {
            self.away = away;
            self.away_secs = 0.0;
        }
    }

    /// Set the Do Not Disturb state. The wire writes and the toast queue's
    /// drain are reconciled by [`advertise_presence`] and the hosts that read
    /// [`is_do_not_disturb`](Self::is_do_not_disturb).
    pub(crate) const fn set_do_not_disturb(&mut self, busy: bool) {
        self.do_not_disturb = busy;
    }

    /// Note user input: reset the idle clock and, once away has held long
    /// enough to be real, clear it (the reference's `MIN_AFK_TIME` debounce).
    fn note_activity(&mut self) {
        if self.away && self.away_secs > MIN_AFK_SECS {
            self.set_away(false);
        }
        self.idle_secs = 0.0;
    }
}

/// The presence plugin: the state, its settings, the AFK clock, the wire
/// advertisement and the IM auto-reply.
pub(crate) struct PresencePlugin;

impl Plugin for PresencePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PresenceState>().add_systems(
            Update,
            (
                track_presence_activity,
                advertise_presence,
                // The reply decision asks whether a conversation with the
                // sender is already open, which must mean "before this IM" —
                // so it runs ahead of the ingest that would open one.
                auto_respond_to_ims.before(crate::conversations::ingest_conversation_events),
            ),
        );
    }
}

/// Advance the idle / away clocks, note any user input, and apply the two
/// timeouts: go away after [`SETTING_AFK_TIMEOUT`] idle seconds, and log out
/// after [`SETTING_QUIT_AFTER_AFK`] away seconds.
///
/// "Input" is any key held or pressed, any mouse button, and any pointer motion
/// or scroll — the same breadth the reference's window handlers cover.
///
/// [`SETTING_AFK_TIMEOUT`]: crate::preferences_general::SETTING_AFK_TIMEOUT
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies: the clock, the four input \
              sources activity is read from, the settings holding both timeouts, identity (no \
              session, no away state), the presence state, and the quit request the \
              quit-after-AFK timeout writes"
)]
fn track_presence_activity(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    settings: Option<Res<ViewerSettings>>,
    identity: Res<SlIdentity>,
    mut state: ResMut<PresenceState>,
    mut quit: MessageWriter<crate::session::QuitRequested>,
) {
    let dt = time.delta_secs();
    state.idle_secs += dt;
    if state.away {
        state.away_secs += dt;
    }
    let active = keys.get_pressed().next().is_some()
        || buttons.get_pressed().next().is_some()
        || motion.delta != Vec2::ZERO
        || scroll.delta != Vec2::ZERO;
    if active {
        state.note_activity();
    }
    // Nothing is timed until there is a session to be away in (the reference's
    // "don't set AFK before a region" guard).
    if identity.agent_id.is_none() {
        state.idle_secs = 0.0;
        return;
    }
    let store = settings.as_deref().map(ViewerSettings::store);
    let afk_timeout = store
        .and_then(|store| {
            store
                .get_u32(crate::preferences_general::SETTING_AFK_TIMEOUT)
                .ok()
        })
        .unwrap_or(0);
    if afk_timeout > 0 && f64::from(state.idle_secs) > f64::from(afk_timeout) && !state.away {
        info!("presence: idle for {afk_timeout}s, going away");
        state.set_away(true);
    }
    let quit_after = store
        .and_then(|store| store.get_u32(SETTING_QUIT_AFTER_AFK).ok())
        .unwrap_or(0);
    if quit_after > 0 && state.away && f64::from(state.away_secs) > f64::from(quit_after) {
        info!("presence: away for {quit_after}s, logging out");
        quit.write(crate::session::QuitRequested);
    }
}

/// Reconcile the wire representation of the two session modes on their edges:
/// start / stop the away and do-not-disturb animations (which is how every
/// other viewer learns the state), and sit / stand the avatar under
/// [`SETTING_SIT_ON_AWAY`].
///
/// The [`ControlFlags::AWAY`](sl_client_bevy::ControlFlags) bit is not sent
/// here — the movement driver owns the control-flag word and folds the away bit
/// into it ([`crate::movement::drive_avatar_controls`]), because a separate
/// writer would just fight it for the same field.
fn advertise_presence(
    settings: Option<Res<ViewerSettings>>,
    agent: Res<SlAgentParcel>,
    mut ground_sit: ResMut<crate::avatar_menu::SelfGroundSit>,
    mut state: ResMut<PresenceState>,
    mut commands: MessageWriter<SlCommand>,
) {
    let away = state.away;
    if away != state.advertised_away {
        if let Some(id) = sl_anim::builtin_animation_by_name(AWAY_ANIMATION) {
            let key = AnimationKey::from(id.id);
            commands.write(SlCommand(if away {
                Command::PlayAnimation(key)
            } else {
                Command::StopAnimation(key)
            }));
        }
        apply_sit_on_away(
            away,
            settings.as_deref(),
            &agent,
            &mut ground_sit,
            &mut state,
            &mut commands,
        );
        state.advertised_away = away;
    }
    let busy = state.do_not_disturb;
    if busy != state.advertised_dnd {
        if let Some(id) = sl_anim::builtin_animation_by_name(DND_ANIMATION) {
            let key = AnimationKey::from(id.id);
            commands.write(SlCommand(if busy {
                Command::PlayAnimation(key)
            } else {
                Command::StopAnimation(key)
            }));
        }
        state.advertised_dnd = busy;
    }
}

/// The reference's `AvatarSitOnAway`: ground-sit on going away and stand back
/// up on returning — but only when *we* sat it down (an avatar that was already
/// sitting on an object when it went away is left exactly where it is).
fn apply_sit_on_away(
    away: bool,
    settings: Option<&ViewerSettings>,
    agent: &SlAgentParcel,
    ground_sit: &mut crate::avatar_menu::SelfGroundSit,
    state: &mut PresenceState,
    commands: &mut MessageWriter<SlCommand>,
) {
    let enabled = settings.is_some_and(|settings| {
        settings
            .store()
            .get_bool(SETTING_SIT_ON_AWAY)
            .unwrap_or(false)
    });
    if away {
        if enabled && agent.seated_on.is_none() && !ground_sit.sitting {
            commands.write(SlCommand(Command::SitOnGround));
            ground_sit.sitting = true;
            state.sat_on_away = true;
        }
    } else if state.sat_on_away {
        // Stand back up whatever the setting says now — leaving the avatar sat
        // down because the preference was switched off mid-away would be worse
        // than honouring the state we created.
        commands.write(SlCommand(Command::Stand));
        ground_sit.sitting = false;
        state.sat_on_away = false;
    }
}

/// Answer an incoming IM with the canned reply of whichever mode is on, once
/// per conversation, and note the reply in that conversation's transcript.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies: the event stream, the presence \
              state, the settings the replies come from, the friend and block lists the \
              decision reads, the contact sets a per-set reply comes from, the conversation \
              model that says whether a session is already open, and the two streams it writes \
              (the wire reply and the transcript notice)"
)]
fn auto_respond_to_ims(
    mut events: MessageReader<SlEvent>,
    presence: Res<PresenceState>,
    settings: Option<Res<ViewerSettings>>,
    friends: Option<Res<crate::people::FriendsModel>>,
    mutes: Option<Res<crate::mutes::MuteModel>>,
    sets: Option<Res<ContactSets>>,
    conversations: Res<ConversationModel>,
    identity: Res<SlIdentity>,
    mut commands: MessageWriter<SlCommand>,
    mut notices: MessageWriter<ConversationNotice>,
) {
    let modes = ReplyModes {
        away: presence.is_away(),
        send_away: bool_setting(settings.as_deref(), SETTING_SEND_AWAY_RESPONSE),
        do_not_disturb: presence.is_do_not_disturb(),
        autorespond: bool_setting(settings.as_deref(), SETTING_AUTORESPOND_MODE),
        autorespond_non_friends: bool_setting(
            settings.as_deref(),
            SETTING_AUTORESPOND_NON_FRIENDS_MODE,
        ),
        send_muted: bool_setting(settings.as_deref(), SETTING_SEND_MUTED_RESPONSE),
    };
    for event in events.read() {
        let SlSessionEvent::InstantMessageReceived(im) = &event.0 else {
            continue;
        };
        // Only a typed 1:1 IM from a live sender is answered: a group or
        // conference line, an offer, another client's auto-reply, and a
        // store-and-forward offline IM all fall through (the reference's
        // `offline == IM_ONLINE` and `from_id.notNull()` guards).
        if im.dialog != ImDialog::Message
            || im.from_group
            || im.offline
            || im.from_agent_id.uuid().is_nil()
            || identity.agent_id == Some(im.from_agent_id)
        {
            continue;
        }
        let is_friend = friends
            .as_deref()
            .is_some_and(|friends| friends.is_friend(im.from_agent_id));
        let is_blocked = mutes
            .as_deref()
            .is_some_and(|mutes| mutes.is_muted(im.from_agent_id.uuid()));
        let Some(mode) = reply_for(modes, is_friend, is_blocked) else {
            continue;
        };
        // Once per conversation, as the reference does: an already-open
        // conversation means this resident has had the reply.
        let key = ConversationKey::Direct(im.from_agent_id);
        if conversations.has_conversation(key) {
            continue;
        }
        let Some(message) =
            reply_text(settings.as_deref(), sets.as_deref(), im.from_agent_id, mode)
        else {
            continue;
        };
        commands.write(SlCommand(Command::AutoResponse {
            to_agent_id: im.from_agent_id,
            message: message.clone(),
        }));
        notices.write(ConversationNotice {
            key,
            body: format!("Autoresponse sent: {message}"),
        });
    }
}

/// A boolean setting, defaulting to off when the store has no answer.
fn bool_setting(settings: Option<&ViewerSettings>, name: &str) -> bool {
    settings.is_some_and(|settings| settings.store().get_bool(name).unwrap_or(false))
}

/// The reply text to answer `sender` with in a resolved [`ReplyMode`]: the
/// override from the smallest contact set they are filed under that carries one,
/// else the configured global text — or `None` when that is empty (a user who
/// blanked the field wants no reply sent).
fn reply_text(
    settings: Option<&ViewerSettings>,
    sets: Option<&ContactSets>,
    sender: sl_client_bevy::AgentKey,
    mode: ReplyMode,
) -> Option<String> {
    if let Some(set_mode) = mode.set_override()
        && let Some(text) = sets.and_then(|sets| sets.autoresponse_for(sender, set_mode))
    {
        return Some(text.to_owned());
    }
    let name = match mode {
        ReplyMode::Muted => SETTING_MUTED_RESPONSE,
        ReplyMode::DoNotDisturb => crate::preferences_chat::SETTING_BUSY_RESPONSE,
        ReplyMode::AutorespondNonFriends => {
            crate::preferences_chat::SETTING_AUTORESPOND_NON_FRIENDS_RESPONSE
        }
        ReplyMode::Autorespond => crate::preferences_chat::SETTING_AUTORESPOND_RESPONSE,
        ReplyMode::Away => SETTING_AWAY_RESPONSE,
    };
    let text = settings?.store().get_str(name).ok()?;
    if text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

/// Toggle a presence mode from the Comm ▸ Online Status menu, raising the
/// reference's "mode is on" notification on the rising edge.
///
/// The two session modes live on [`PresenceState`]; the two autorespond modes
/// are persisted settings, so toggling those writes the store (and saves it) —
/// which is also what makes them survive a relog.
pub(crate) fn toggle_presence_mode(
    action: &str,
    state: &mut PresenceState,
    settings: &mut ViewerSettings,
    notify: &mut MessageWriter<ShowNotification>,
) -> bool {
    match action {
        "presence-away" => {
            state.set_away(!state.is_away());
        }
        "presence-do-not-disturb" => {
            let on = !state.is_do_not_disturb();
            state.set_do_not_disturb(on);
            if on {
                notify.write(ShowNotification::new("DoNotDisturbModeSet"));
            }
        }
        "presence-autorespond" => {
            toggle_mode_setting(
                SETTING_AUTORESPOND_MODE,
                "AutorespondModeSet",
                settings,
                notify,
            );
        }
        "presence-autorespond-non-friends" => {
            toggle_mode_setting(
                SETTING_AUTORESPOND_NON_FRIENDS_MODE,
                "AutorespondNonFriendsModeSet",
                settings,
                notify,
            );
        }
        _ => return false,
    }
    true
}

/// Flip a persisted mode flag and, on the rising edge, raise its notification.
fn toggle_mode_setting(
    name: &str,
    template: &'static str,
    settings: &mut ViewerSettings,
    notify: &mut MessageWriter<ShowNotification>,
) {
    let on = !settings.store().get_bool(name).unwrap_or(false);
    settings.set_account(name, SettingValue::Bool(on));
    settings.save_async();
    if on {
        notify.write(ShowNotification::new(template));
    }
}

/// Whether the own name tag should carry the reference's `Auto-Response` status
/// entry: either autorespond mode is on and the tag is set to show it.
#[must_use]
pub(crate) fn shows_autoresponse(settings: Option<&ViewerSettings>) -> bool {
    bool_setting(settings, SETTING_AUTORESPOND_MODE)
        || bool_setting(settings, SETTING_AUTORESPOND_NON_FRIENDS_MODE)
}

/// The agent a [`ConversationKey::Direct`] names, for a caller that only has
/// the key — used by the tests below to assert the reply is addressed to the
/// sender.
#[cfg(test)]
const fn direct_peer(key: ConversationKey) -> Option<sl_client_bevy::AgentKey> {
    match key {
        ConversationKey::Direct(agent) => Some(agent),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AWAY_ANIMATION, ContactSets, DND_ANIMATION, MIN_AFK_SECS, PresenceState, ReplyMode,
        ReplyModes, SetAutoresponseMode, direct_peer, reply_for,
    };
    use crate::conversations::ConversationKey;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::AgentKey;

    /// Both presence animations resolve in the built-in registry — without them
    /// no other viewer could ever learn the state.
    #[test]
    fn presence_animations_are_builtins() {
        assert!(
            sl_anim::builtin_animation_by_name(AWAY_ANIMATION).is_some(),
            "ANIM_AGENT_AWAY is a built-in"
        );
        assert!(
            sl_anim::builtin_animation_by_name(DND_ANIMATION).is_some(),
            "ANIM_AGENT_DO_NOT_DISTURB is a built-in"
        );
    }

    /// A blocked sender short-circuits the mode chain: the blocked reply when
    /// it is enabled, silence when it is not — never the busy / away text.
    #[test]
    fn a_blocked_sender_never_reaches_the_mode_chain() {
        let modes = ReplyModes {
            do_not_disturb: true,
            send_muted: true,
            ..ReplyModes::default()
        };
        assert_eq!(reply_for(modes, false, true), Some(ReplyMode::Muted));
        let silent = ReplyModes {
            send_muted: false,
            ..modes
        };
        assert_eq!(reply_for(silent, false, true), None);
        // Unblocked, the same modes answer with the busy reply.
        assert_eq!(
            reply_for(silent, false, false),
            Some(ReplyMode::DoNotDisturb)
        );
    }

    /// The mode precedence is the reference's: Do Not Disturb, then
    /// autorespond-to-non-friends (only for a non-friend), then autorespond,
    /// then away.
    #[test]
    fn mode_precedence_matches_the_reference() {
        let all = ReplyModes {
            away: true,
            send_away: true,
            do_not_disturb: true,
            autorespond: true,
            autorespond_non_friends: true,
            send_muted: false,
        };
        assert_eq!(reply_for(all, false, false), Some(ReplyMode::DoNotDisturb));
        let no_dnd = ReplyModes {
            do_not_disturb: false,
            ..all
        };
        assert_eq!(
            reply_for(no_dnd, false, false),
            Some(ReplyMode::AutorespondNonFriends)
        );
        // A friend skips the non-friends mode and lands on plain autorespond.
        assert_eq!(reply_for(no_dnd, true, false), Some(ReplyMode::Autorespond));
        let only_away = ReplyModes {
            autorespond: false,
            autorespond_non_friends: false,
            ..no_dnd
        };
        assert_eq!(reply_for(only_away, true, false), Some(ReplyMode::Away));
        // Away alone, without the away-reply toggle, answers nothing.
        let quiet_away = ReplyModes {
            send_away: false,
            ..only_away
        };
        assert_eq!(reply_for(quiet_away, true, false), None);
    }

    /// With no mode on at all, an ordinary IM gets no reply.
    #[test]
    fn no_mode_no_reply() {
        assert_eq!(reply_for(ReplyModes::default(), true, false), None);
        assert_eq!(reply_for(ReplyModes::default(), false, false), None);
    }

    /// Input clears a settled away state but is swallowed while it is younger
    /// than the debounce — the reference's `MIN_AFK_TIME` rule, which keeps the
    /// mouse move that lands right after the auto-AFK from undoing it.
    #[test]
    fn away_clears_only_after_the_debounce() {
        let mut state = PresenceState::default();
        state.set_away(true);
        state.away_secs = MIN_AFK_SECS / 2.0;
        state.note_activity();
        assert!(
            state.is_away(),
            "a twitch inside the debounce keeps away on"
        );
        assert!(
            state.idle_secs.abs() < f32::EPSILON,
            "but it still resets the idle clock"
        );
        state.away_secs = MIN_AFK_SECS + 1.0;
        state.note_activity();
        assert!(!state.is_away(), "past the debounce, input clears away");
    }

    /// Setting away restarts the away clock, so the quit-after-AFK timeout
    /// measures this away spell rather than an older one.
    #[test]
    fn the_away_clock_restarts_on_each_edge() {
        let mut state = PresenceState::default();
        state.set_away(true);
        state.away_secs = 120.0;
        state.set_away(false);
        assert!(
            state.away_secs.abs() < f32::EPSILON,
            "the away clock restarted"
        );
        state.away_secs = 30.0;
        // A redundant set is not an edge and must not reset the clock.
        state.set_away(false);
        assert!(
            (state.away_secs - 30.0).abs() < f32::EPSILON,
            "a redundant set left the away clock alone"
        );
    }

    /// Only the three *mode* replies can be overridden per contact set: the away
    /// and blocked texts are statements about the user, not about the sender.
    #[test]
    fn only_the_mode_replies_have_a_per_set_layer() {
        assert_eq!(
            ReplyMode::DoNotDisturb.set_override(),
            Some(SetAutoresponseMode::Busy)
        );
        assert_eq!(
            ReplyMode::Autorespond.set_override(),
            Some(SetAutoresponseMode::Autorespond)
        );
        assert_eq!(
            ReplyMode::AutorespondNonFriends.set_override(),
            Some(SetAutoresponseMode::NonFriends)
        );
        assert_eq!(ReplyMode::Away.set_override(), None);
        assert_eq!(ReplyMode::Muted.set_override(), None);
    }

    /// A contact set's own reply is answered with in place of the global one —
    /// and with no settings store at all, which is what proves the per-set text
    /// is consulted *before* the global reply rather than after it.
    #[test]
    fn a_per_set_reply_answers_before_the_global_one()
    -> Result<(), crate::contact_sets::ContactSetRefusal> {
        let sender = AgentKey::from(sl_client_bevy::Uuid::from_u128(0x7));
        let mut sets = ContactSets::default();
        sets.create_set("Partner")?;
        sets.add_member("Partner", sender, "Alpha Resident")?;
        sets.set_autoresponse(
            "Partner",
            SetAutoresponseMode::Busy,
            true,
            "back in five minutes",
        )?;
        assert_eq!(
            super::reply_text(None, Some(&sets), sender, ReplyMode::DoNotDisturb).as_deref(),
            Some("back in five minutes")
        );
        // A mode this set does not override has no answer without a store.
        assert_eq!(
            super::reply_text(None, Some(&sets), sender, ReplyMode::Autorespond),
            None
        );
        // Nor does someone the set has never heard of.
        let stranger = AgentKey::from(sl_client_bevy::Uuid::from_u128(0x8));
        assert_eq!(
            super::reply_text(None, Some(&sets), stranger, ReplyMode::DoNotDisturb),
            None
        );
        Ok(())
    }

    /// A direct conversation key names the resident the reply is addressed to.
    #[test]
    fn the_reply_targets_the_sender() {
        let sender = AgentKey::from(sl_client_bevy::Uuid::from_u128(0x42));
        assert_eq!(
            direct_peer(ConversationKey::Direct(sender)),
            Some(sender),
            "the notice lands in the sender's own conversation"
        );
        assert_eq!(direct_peer(ConversationKey::Nearby), None);
    }
}
