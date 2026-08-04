//! The agent's mute (block) list, mirrored viewer-side.
//!
//! The viewer has long *written* mutes (`Command::Mute` / `Command::Unmute`
//! from the avatar/object menus) but never read the list back; the name-tag
//! colouring (a muted avatar's tag goes grey, the reference's `NameTagMuted`)
//! needs an `is_muted` query. This module requests the list once per session
//! (`RequestMuteList` — the reply arrives via the Xfer path as
//! [`Event::MuteList`](sl_client_bevy::SlSessionEvent::MuteList)) and keeps
//! the set current across local mute/unmute actions by observing the outgoing
//! [`SlCommand`] stream ([`note_local_mutes`]), so the tag colour flips
//! immediately without a re-request round trip — and without every menu that
//! can mute having to know about this model.

use bevy::prelude::*;
use std::collections::HashSet;

use sl_client_bevy::{Command, MuteType, SlCommand, SlEvent, SlIdentity, SlSessionEvent, Uuid};

/// The agent's mute list: every muted id (agents and objects alike — the tag
/// colouring only ever looks up agent ids).
#[derive(Resource, Debug, Default)]
pub(crate) struct MuteModel {
    /// The muted ids, replaced wholesale on each [`SlSessionEvent::MuteList`].
    muted: HashSet<Uuid>,
    /// Whether the one-per-session `RequestMuteList` has been sent.
    requested: bool,
}

impl MuteModel {
    /// Whether `id` is on the mute list.
    pub(crate) fn is_muted(&self, id: Uuid) -> bool {
        self.muted.contains(&id)
    }

    /// Record a locally-issued mute so consumers update without waiting for a
    /// list re-request.
    pub(crate) fn note_mute(&mut self, id: Uuid) {
        self.muted.insert(id);
    }

    /// Record a locally-issued unmute (see [`Self::note_mute`]).
    pub(crate) fn note_unmute(&mut self, id: Uuid) {
        self.muted.remove(&id);
    }
}

/// Request the mute list once the session is up (the login handshake has
/// produced an agent id).
pub(crate) fn request_mute_list(
    identity: Res<SlIdentity>,
    mut model: ResMut<MuteModel>,
    mut commands: MessageWriter<SlCommand>,
) {
    if model.requested || identity.agent_id.is_none() {
        return;
    }
    model.requested = true;
    commands.write(SlCommand(Command::RequestMuteList));
}

/// Fold a received mute list into the model (`MuteList` replaces the set;
/// `MuteListUnchanged` means the cached copy the request named is current —
/// nothing to do, and the locally-noted entries stay).
pub(crate) fn ingest_mute_list(mut events: MessageReader<SlEvent>, mut model: ResMut<MuteModel>) {
    for event in events.read() {
        if let SlSessionEvent::MuteList(entries) = &event.0 {
            model.muted = entries.iter().map(|entry| entry.id).collect();
        }
    }
}

/// Mirror locally-issued avatar mutes/unmutes into the model by watching the
/// outgoing command stream (every mute menu writes an [`SlCommand`], so no
/// call site needs to know this model exists).
pub(crate) fn note_local_mutes(
    mut outgoing: MessageReader<SlCommand>,
    mut model: ResMut<MuteModel>,
) {
    for command in outgoing.read() {
        match &command.0 {
            Command::Mute {
                id,
                mute_type: MuteType::Agent,
                ..
            } => {
                model.note_mute(*id);
            }
            Command::Unmute { id, .. } => {
                model.note_unmute(*id);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MuteModel;
    use sl_client_bevy::Uuid;

    /// Local notes flip membership immediately; a list ingest replaces it.
    #[test]
    fn notes_and_replacement() {
        let mut model = MuteModel::default();
        let troll = Uuid::from_u128(0xBAD);
        assert!(!model.is_muted(troll));
        model.note_mute(troll);
        assert!(model.is_muted(troll));
        model.note_unmute(troll);
        assert!(!model.is_muted(troll));
    }
}
