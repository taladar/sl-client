//! The agent's mute (block) list.
//!
//! The pure [`MuteList`] the session runtime also keeps, plus the one piece of
//! state only a viewer has: whether it has already asked the grid for the
//! list.

use core::time::Duration;

use bevy::prelude::*;
use sl_client_bevy::{MuteEntry, MuteList, Uuid};

/// How long the list is waited for before what depends on it goes ahead
/// without it: the reference's `LLMuteList::updateLoadState`, which gives up
/// on the request after 30 seconds.
pub const MUTE_LIST_WAIT: Duration = Duration::from_secs(30);

/// The agent's mute list as a Bevy resource: the pure [`MuteList`] the session
/// runtime also keeps, plus the one piece of state only a viewer has — whether
/// it has already asked the grid for the list.
///
/// The matching rules (the by-name fallback, the per-aspect exception bits,
/// [`chat_text_muted`](sl_client_bevy::chat_text_muted)) live with the list in
/// `sl-proto`, so the viewer's
/// surfaces and the runtime's chat-log transcript answer the same question the
/// same way rather than each spelling it out.
#[derive(Resource, Debug, Default)]
pub struct MuteModel {
    /// The list itself, mirrored from the received
    /// [`MuteList`](sl_client_bevy::SlSessionEvent::MuteList) event and the
    /// outgoing mute/unmute commands.
    list: MuteList,
    /// When the one-per-session `RequestMuteList` was sent (the app's
    /// elapsed time), `None` before.
    requested_at: Option<Duration>,
    /// Whether the grid has answered the request: with the list, with "the
    /// cached one is current", or with a file that would not parse.
    answered: bool,
}

impl MuteModel {
    /// Claim the one-per-session `RequestMuteList` slot at `now` (the app's
    /// elapsed time): true the first time it is called, false every time
    /// after. The latch lives with the model so a second requester cannot race
    /// a duplicate request onto the wire.
    pub const fn claim_request(&mut self, now: Duration) -> bool {
        if self.requested_at.is_some() {
            return false;
        }
        self.requested_at = Some(now);
        true
    }

    /// Record that the grid answered the request, however it answered.
    pub const fn note_answered(&mut self) {
        self.answered = true;
    }

    /// Whether the list is as loaded as it will get at `now`: answered, or
    /// asked for [`MUTE_LIST_WAIT`] ago without an answer — the reference's
    /// `LLMuteList::updateLoadState`, which what acts on a sender's mute
    /// (the read of the kept messages) waits for.
    #[must_use]
    pub fn is_settled(&self, now: Duration) -> bool {
        self.answered
            || self
                .requested_at
                .is_some_and(|at| now.saturating_sub(at) >= MUTE_LIST_WAIT)
    }

    /// The list itself — the whole pure model, for the callers that ask it a
    /// question this resource does not forward (notably
    /// [`chat_text_muted`](sl_client_bevy::chat_text_muted)).
    #[must_use]
    pub const fn list(&self) -> &MuteList {
        &self.list
    }

    /// Whether `id` is on the mute list at all (any aspect)
    /// ([`MuteList::is_muted`]).
    #[must_use]
    pub fn is_muted(&self, id: Uuid) -> bool {
        self.list.is_muted(id)
    }

    /// Whether the aspect whose *exception* bit is `allow_mask` is actually
    /// muted for `id` ([`MuteList::is_muted_aspect`]).
    #[must_use]
    pub fn is_muted_aspect(&self, id: Uuid, allow_mask: u32) -> bool {
        self.list.is_muted_aspect(id, allow_mask)
    }

    /// [`Self::is_muted_aspect`] widened with the reference's by-name fallback
    /// ([`MuteList::is_muted_aspect_named`]).
    #[must_use]
    pub fn is_muted_aspect_named(&self, id: Uuid, name: &str, allow_mask: u32) -> bool {
        self.list.is_muted_aspect_named(id, name, allow_mask)
    }

    /// Whether the resident `id` / `name` names has their text chat blocked
    /// ([`MuteList::text_muted`]).
    #[must_use]
    pub fn text_muted(&self, id: Uuid, name: &str) -> bool {
        self.list.text_muted(id, name)
    }

    /// The whole list, in display order ([`MuteList::entries`]).
    #[must_use]
    pub fn entries(&self) -> &[MuteEntry] {
        self.list.entries()
    }

    /// The list revision — a view stores the value it last built at and
    /// rebuilds when it advances ([`MuteList::revision`]).
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.list.revision()
    }

    /// Whether the list is at [`MUTE_LIST_LIMIT`](sl_client_bevy::MUTE_LIST_LIMIT)
    /// and refuses further mutes ([`MuteList::is_full`]).
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.list.is_full()
    }

    /// Whether a **by-name** entry already carries `name`
    /// ([`MuteList::has_by_name`]).
    #[must_use]
    pub fn has_by_name(&self, name: &str) -> bool {
        self.list.has_by_name(name)
    }

    /// The entry matching `id` / `name`, if any ([`MuteList::entry`]).
    #[must_use]
    pub fn entry(&self, id: Uuid, name: &str) -> Option<&MuteEntry> {
        self.list.entry(id, name)
    }

    /// Record a locally-issued mute ([`MuteList::note_mute`]).
    pub fn note_mute(&mut self, entry: MuteEntry) {
        self.list.note_mute(entry);
    }

    /// Record a locally-issued unmute ([`MuteList::note_unmute`]).
    pub fn note_unmute(&mut self, id: Uuid, name: &str) {
        self.list.note_unmute(id, name);
    }

    /// Replace the whole list, a received `MuteList` ([`MuteList::replace`]).
    pub fn replace(&mut self, entries: Vec<MuteEntry>) {
        self.list.replace(entries);
    }
}

#[cfg(test)]
mod tests {
    use super::{MUTE_LIST_WAIT, MuteModel};
    use core::time::Duration;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{MuteEntry, MuteFlags, MuteType, Uuid};

    /// The resource forwards to the pure list it wraps: a locally-noted mute
    /// is visible through every accessor a surface uses, and the revision the
    /// block-list view rebuilds on advances with it.
    #[test]
    fn the_resource_forwards_to_the_list_it_wraps() {
        let troll = Uuid::from_u128(0x11);
        let mut model = MuteModel::default();
        let before = model.revision();
        model.note_mute(MuteEntry {
            id: troll,
            name: "Troll Resident".to_owned(),
            mute_type: MuteType::Agent,
            flags: MuteFlags::default(),
        });

        assert_eq!(model.is_muted(troll), true);
        assert_eq!(model.text_muted(troll, "Troll Resident"), true);
        assert_eq!(model.entries().len(), 1);
        assert_eq!(model.entry(troll, "").is_some(), true);
        assert_eq!(model.is_full(), false);
        assert_eq!(model.list().is_muted(troll), true);
        assert_eq!(model.revision() > before, true, "the view must rebuild");
    }

    /// The one-per-session `RequestMuteList` latch — the only state the
    /// resource adds to the list — hands the slot out exactly once.
    #[test]
    fn the_request_latch_is_claimed_once() {
        let mut model = MuteModel::default();
        assert_eq!(model.claim_request(Duration::from_secs(1)), true);
        assert_eq!(model.claim_request(Duration::from_secs(2)), false);
    }

    /// The list is settled once answered, or once it has been waited for as
    /// long as the reference waits; never before it is asked for.
    #[test]
    fn the_list_settles_on_an_answer_or_after_the_wait() {
        let start = Duration::from_secs(5);
        let mut model = MuteModel::default();
        assert_eq!(model.is_settled(start), false);
        model.claim_request(start);
        assert_eq!(model.is_settled(start), false);
        assert_eq!(model.is_settled(start + MUTE_LIST_WAIT), true);
        let mut answered = MuteModel::default();
        answered.claim_request(start);
        answered.note_answered();
        assert_eq!(answered.is_settled(start), true);
    }
}
