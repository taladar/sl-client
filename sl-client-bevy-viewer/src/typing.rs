//! Local-chat typing state for the **own** avatar (P31.9): plays
//! `ANIM_AGENT_TYPE` — the reference viewer's hands-on-keyboard gesture — locally
//! while the user is entering local chat and broadcasts a `StartTyping` /
//! `StopTyping` `ChatFromViewer` so nearby avatars see the same state.
//!
//! On the wire, typing is *two* independent signals — the reference viewer's
//! `LLAgent::startTyping` / `stopTyping` send both:
//!
//! 1. An **`AgentAnimation`** request that starts / stops `ANIM_AGENT_TYPE` on the
//!    own avatar. This is what makes *other* viewers show the typing animation: the
//!    simulator rebroadcasts it to nearby avatars as an `AvatarAnimation`, which the
//!    Phase 18 pipeline already ingests and plays like any other keyframe (the
//!    animation is a downloadable built-in). The simulator does **not** synthesise
//!    the typing animation itself — it only relays the requesting client's
//!    `AgentAnimation` — so this request is required for anyone else to see it.
//! 2. An empty-text **`ChatFromViewer`** with a `StartTyping` / `StopTyping` type.
//!    The simulator relays it to neighbours as a `ChatFromSimulator` the P11.1
//!    ingest surfaces as an
//!    [`Event::ChatTyping`](sl_client_bevy::SlSessionEvent::ChatTyping) — the "is
//!    typing" chat indicator, distinct from and not the trigger for the animation.
//!
//! A neighbour's typing therefore needs no receive-side code here: their client
//! sends the `AgentAnimation`, the simulator broadcasts it, and Phase 18 plays it.
//! This module adds the own-avatar halves — send both signals on the typing edge,
//! and (for immediate feedback / an OpenSim child presence the simulator would not
//! echo, exactly as P31.6 does for locomotion) also play the animation locally.
//!
//! Typing is a sibling of the P31.6 locomotion state animations — an
//! activity-driven state animation, not a procedural adjuster — but it is an
//! **overlay**: it plays concurrently with stand / walk rather than replacing the
//! locomotion state, so it drives a dedicated slot on
//! [`AnimationPlayback`](crate::animations::AnimationPlayback::set_client_typing)
//! that the pose merge blends against the locomotion / simulator set by priority.
//!
//! There is no chat-entry box in the viewer yet (the read-only Phase 11
//! [overlay](crate::chat) has no input, and a full input UI is its own roadmap
//! task), so the typing state stands in for "the chat bar is open and being typed
//! into": the **T** key toggles it. The state lives on a [`TypingState`] resource
//! with a public setter, so once a real chat input arrives it drives the same
//! state — start typing when the bar gains a character, stop on send / close —
//! without touching this driver.
//!
//! The typing UI *sound* the reference viewer also plays is deliberately left out:
//! the viewer has no sound-effect playback yet (that is a separate roadmap task),
//! so only the animation and the wire signal are implemented here.

use bevy::prelude::*;
use sl_client_bevy::{AnimationKey, AssetKey, Command, SlCommand, SlIdentity};

use crate::animations::{AnimationManager, AnimationPlayback};
use crate::avatars::AvatarState;

/// The short name of the built-in `ANIM_AGENT_TYPE` animation in the [`sl_anim`]
/// registry — the hands-on-keyboard gesture played and requested while typing.
const TYPE_ANIMATION: &str = "type";

/// Whether the own avatar is currently typing into local chat — driven by the
/// nearby-chat bar ([`crate::nearby_chat_bar`]) through [`set`](Self::set): active
/// while the bar is focused and holds a draft, inactive on send / blur.
#[derive(Resource, Default)]
pub(crate) struct TypingState {
    /// Whether typing is active this frame.
    active: bool,
    /// The `active` value last advertised to the simulator, so a `StartTyping` /
    /// `StopTyping` `ChatFromViewer` is emitted only on the *edge* rather than every
    /// frame — the simulator holds the state until the opposite signal arrives.
    advertised: bool,
}

impl TypingState {
    /// Whether the own avatar is typing this frame.
    #[must_use]
    pub(crate) const fn is_active(&self) -> bool {
        self.active
    }

    /// Set the typing state (the nearby-chat bar calls this while a draft is being
    /// typed, and clears it on send / blur). The wire edge is reconciled by
    /// [`drive_own_typing`], so this only records intent.
    pub(crate) const fn set(&mut self, active: bool) {
        self.active = active;
    }
}

/// Drive the own avatar's typing state each frame (P31.9): on the typing edge, send
/// both wire signals — an `AgentAnimation` request that starts / stops
/// `ANIM_AGENT_TYPE` (so the simulator rebroadcasts it and *other* viewers animate
/// the typing) and a `StartTyping` / `StopTyping` `ChatFromViewer` (the "is typing"
/// indicator) — while also playing the animation locally for immediate own-avatar
/// feedback. The state itself is set by the nearby-chat bar
/// ([`crate::nearby_chat_bar`]); this reconciles the edge from it.
///
/// The wire signals are sent regardless of the own avatar's render state, since
/// they are what let *other* clients see the typing; the local play is gated on the
/// own avatar being **rigged** (there is a skeleton to pose — a placeholder sphere
/// gains nothing). On a root presence the simulator echoes the `AgentAnimation`
/// back as an `AvatarAnimation` the Phase 18 path also plays, but they share the
/// one `ANIM_AGENT_TYPE` id so the pose merge collapses them to a single motion
/// rather than doubling.
pub(crate) fn drive_own_typing(
    time: Res<Time>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    mut state: ResMut<TypingState>,
    mut manager: ResMut<AnimationManager>,
    mut playback: ResMut<AnimationPlayback>,
    mut writer: MessageWriter<SlCommand>,
) {
    let now = time.elapsed_secs();
    let active = state.is_active();
    let type_id = sl_anim::builtin_animation_by_name(TYPE_ANIMATION).map(|builtin| builtin.id);

    // On the edge only — the simulator holds each state between signals, so
    // re-sending every frame would flood the circuit — advertise the typing state:
    // the `AgentAnimation` request that drives the animation on other viewers and
    // the `ChatFromViewer` "is typing" indicator.
    if active != state.advertised {
        if let Some(id) = type_id {
            let anim = AnimationKey::from(id);
            writer.write(SlCommand(if active {
                Command::PlayAnimation(anim)
            } else {
                Command::StopAnimation(anim)
            }));
        }
        writer.write(SlCommand(Command::Typing(active)));
        state.advertised = active;
        if std::env::var("SL_VIEWER_LOG_TYPING").as_deref() == Ok("1") {
            info!("P31.9 own typing -> {active}");
        }
    }

    let Some(own) = identity.agent_id else {
        return;
    };
    // Play `ANIM_AGENT_TYPE` locally for immediate feedback while typing, but only on
    // a rigged own avatar (a placeholder sphere has no skeleton to pose); ease it out
    // otherwise.
    let desired = if active && avatars.joint_entities_of(own).is_some() {
        type_id
    } else {
        None
    };
    if let Some(id) = desired {
        manager.request(AssetKey::from(id));
    }
    playback.set_client_typing(own, desired, now);
}

#[cfg(test)]
mod tests {
    use super::{TYPE_ANIMATION, TypingState};

    /// The driver's [`TYPE_ANIMATION`] lookup resolves to a downloadable built-in
    /// (`ANIM_AGENT_TYPE`), so its asset can be requested and played locally.
    #[test]
    fn type_animation_is_a_downloadable_builtin() -> Result<(), &'static str> {
        let builtin =
            sl_anim::builtin_animation_by_name(TYPE_ANIMATION).ok_or("type is a built-in")?;
        assert!(
            builtin.is_downloadable(),
            "ANIM_AGENT_TYPE is a keyframe asset the viewer fetches"
        );
        Ok(())
    }

    /// The state setter records intent; the wire edge is reconciled separately by
    /// the driver, so the setter itself just mirrors the requested value.
    #[test]
    fn set_records_intent() {
        let mut state = TypingState::default();
        assert!(!state.is_active());
        state.set(true);
        assert!(state.is_active());
        state.set(false);
        assert!(!state.is_active());
    }
}
