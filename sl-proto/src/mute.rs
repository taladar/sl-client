//! The agent's **mute (block) list** as a pure model — the list itself plus the
//! questions every surface asks of it.
//!
//! The wire type is [`MuteEntry`]: the file the simulator serves is parsed
//! into those values and surfaced as
//! [`Event::MuteList`](crate::Event::MuteList). What was missing was anything
//! that *held* them, so each consumer kept its own copy and restated the
//! matching rule. [`MuteList`] is that copy, and [`Session`](crate::Session)
//! owns one ([`Session::mutes`](crate::Session::mutes)) fed by the received
//! list and by the mutes it sends, so every runtime consumer — the chat-log
//! transcript among them — gets the same answer as the viewer's own model
//! without a cross-tier snapshot to go stale.
//!
//! Grounded in Firestorm's `LLMuteList` (`llmutelist.cpp`): the id lookup with
//! a legacy by-name fallback (`isMuted(id, name, flags)`), the per-aspect
//! *exception* bits, and the `MuteListLimit` cap.

use crate::types::{ChatMessage, ChatSource, Event, ImDialog, MuteEntry, MuteFlags};
use std::collections::BTreeSet;
use uuid::Uuid;

/// The most entries the mute list holds — the reference's `MuteListLimit`
/// debug setting, whose default this matches. A mute past the limit is refused
/// client-side (the server silently drops it).
pub const MUTE_LIST_LIMIT: usize = 1000;

/// The agent's mute list: every muted entry (agents and objects alike), the
/// derived id index behind the hot `is_muted` query, and a revision stamp a
/// view rebuilds on.
///
/// Entries, not just ids: a block-list surface lists and edits the name, the
/// [`MuteType`](crate::MuteType) and the per-aspect [`MuteFlags`] exceptions,
/// so the whole entry is kept and the id set is derived from it.
#[derive(Debug, Default)]
pub struct MuteList {
    /// The entries, in the order the list was received / mutes were added.
    entries: Vec<MuteEntry>,
    /// The non-nil muted ids, derived from [`Self::entries`] — the hot-path
    /// [`Self::is_muted`] index.
    muted: BTreeSet<Uuid>,
    /// Bumped on every change to [`Self::entries`], so a view rebuilds exactly
    /// when the list actually moved.
    revision: u64,
}

impl MuteList {
    /// An empty list (the state before one is requested).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            muted: BTreeSet::new(),
            revision: 0,
        }
    }

    /// Whether `id` is on the mute list at all (any aspect).
    #[must_use]
    pub fn is_muted(&self, id: Uuid) -> bool {
        self.muted.contains(&id)
    }

    /// Whether the aspect whose *exception* bit is `allow_mask` (one of the
    /// `MuteFlags::ALLOW_*` constants) is actually muted for `id`: the id is on
    /// the list **and** the entry does not carry that exception.
    #[must_use]
    pub fn is_muted_aspect(&self, id: Uuid, allow_mask: u32) -> bool {
        self.entries
            .iter()
            .any(|entry| !entry.id.is_nil() && entry.id == id && !entry.flags.contains(allow_mask))
    }

    /// [`Self::is_muted_aspect`] widened with the reference's **by-name**
    /// fallback: `LLMuteList::isMuted(id, name, flags)` looks the id up first
    /// and, failing that, consults the legacy by-name set.
    ///
    /// A [`MuteType::ByName`](crate::MuteType::ByName) entry is what the *Block
    /// object by name…* dialog writes, and it is the only lever there is
    /// against a spammy object one cannot click — a griefer's rezzer hands out
    /// a fresh id per object, so matching on the id alone would leave that
    /// dialog inert for the very case it exists for. An empty `name` never
    /// matches, so a caller with no name to offer degrades to the id-only test
    /// rather than to "mute everything blocked by name".
    #[must_use]
    pub fn is_muted_aspect_named(&self, id: Uuid, name: &str, allow_mask: u32) -> bool {
        self.is_muted_aspect(id, allow_mask)
            || (!name.is_empty()
                && self.entries.iter().any(|entry| {
                    entry.id.is_nil()
                        && entry.name.eq_ignore_ascii_case(name)
                        && !entry.flags.contains(allow_mask)
                }))
    }

    /// Whether the resident (or object) `id` / `name` names has their **text
    /// chat** blocked — the reference's `isMuted(id, name, LLMute::flagTextChat)`,
    /// the one question every conversation surface asks of the list, stated
    /// once so the overlay, the conversation tabs and the disk transcript
    /// cannot drift apart. An empty `name` falls back to matching by id alone.
    #[must_use]
    pub fn text_muted(&self, id: Uuid, name: &str) -> bool {
        self.is_muted_aspect_named(id, name, MuteFlags::ALLOW_TEXT_CHAT)
    }

    /// Whether `event` carries **text chat** from a blocked speaker — the one
    /// question a surface asks of an arriving event before showing it or
    /// writing it down, over the four events that carry a line someone said:
    /// nearby chat, a 1:1 instant message, a group line and a conference line.
    ///
    /// Every other event answers `false`, including those a blocked resident
    /// may also have caused (a group-session invitation, an inventory offer):
    /// they carry no line, and what to do about them is the consuming
    /// surface's decision, not this one's. The nearby-chat case is
    /// [`chat_text_muted`], object owner and all.
    #[must_use]
    pub fn text_muted_event(&self, event: &Event) -> bool {
        match event {
            Event::ChatReceived(chat) => chat_text_muted(self, chat),
            Event::InstantMessageReceived(im) if im.dialog == ImDialog::Message => {
                self.text_muted(im.from_agent_id.uuid(), &im.from_agent_name)
            }
            Event::GroupSessionMessage {
                from_agent_id,
                from_name,
                ..
            }
            | Event::ConferenceSessionMessage {
                from_agent_id,
                from_name,
                ..
            } => self.text_muted(from_agent_id.uuid(), from_name),
            _other => false,
        }
    }

    /// The whole list, in display order.
    #[must_use]
    pub fn entries(&self) -> &[MuteEntry] {
        &self.entries
    }

    /// The list revision — a view stores the value it last built at and
    /// rebuilds when it advances.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the list is at [`MUTE_LIST_LIMIT`] and refuses further mutes.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.entries.len() >= MUTE_LIST_LIMIT
    }

    /// Whether a **by-name** entry already carries `name` (case-insensitively)
    /// — the duplicate check a by-name block needs, since such entries share a
    /// nil id and nothing else tells them apart. Entries with an id are not
    /// consulted: the reference keeps its by-name mutes in a separate set, so
    /// blocking an object *by name* is allowed even when a same-named avatar is
    /// blocked by id.
    #[must_use]
    pub fn has_by_name(&self, name: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.id.is_nil() && entry.name.eq_ignore_ascii_case(name))
    }

    /// The entry matching `id` / `name`, if any: by id when the id is
    /// non-nil, else by case-folded name (a by-name entry).
    #[must_use]
    pub fn entry(&self, id: Uuid, name: &str) -> Option<&MuteEntry> {
        self.entries
            .iter()
            .find(|entry| same_target(entry, id, name))
    }

    /// Record a locally-issued mute so consumers update without waiting for a
    /// list re-request. An existing entry for the same target is **replaced**
    /// (that is how a flag edit lands, since it re-sends the whole entry).
    pub fn note_mute(&mut self, entry: MuteEntry) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|candidate| same_target(candidate, entry.id, &entry.name))
        {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }
        self.reindex();
    }

    /// Record a locally-issued unmute (see [`Self::note_mute`]).
    pub fn note_unmute(&mut self, id: Uuid, name: &str) {
        self.entries.retain(|entry| !same_target(entry, id, name));
        self.reindex();
    }

    /// Replace the whole list (a received
    /// [`Event::MuteList`](crate::Event::MuteList)).
    pub fn replace(&mut self, entries: Vec<MuteEntry>) {
        self.entries = entries;
        self.reindex();
    }

    /// Rebuild the derived id index and bump the revision.
    fn reindex(&mut self) {
        self.muted = self
            .entries
            .iter()
            .map(|entry| entry.id)
            .filter(|id| !id.is_nil())
            .collect();
        self.revision = self.revision.wrapping_add(1);
    }
}

/// Whether `entry` is the mute list's record of `id` / `name`: by id when the
/// id is non-nil, else by case-folded name (a
/// [`MuteType::ByName`](crate::MuteType::ByName) entry). A caller often knows
/// only a partial name (an object mute recorded before the properties reply
/// landed), so matching a non-nil id ignores the name deliberately.
fn same_target(entry: &MuteEntry, id: Uuid, name: &str) -> bool {
    if id.is_nil() {
        entry.id.is_nil() && entry.name.eq_ignore_ascii_case(name)
    } else {
        entry.id == id
    }
}

/// Whether a nearby-chat message must be swallowed because its speaker's **text
/// chat** is blocked — the reference's pair of `flagTextChat` tests in
/// `LLViewerMessage`'s `process_chat_from_simulator`:
/// `isMuted(from_id, from_name, flagTextChat) || isMuted(owner_id,
/// flagTextChat)`.
///
/// An object is muted by *either* its own id or its owner's, exactly as a
/// sound from it is: blocking the resident who rezzed a chatspammer has to
/// silence every one of their objects, not just the one that was clickable.
/// The owner is not consulted for an avatar speaker — an avatar owns itself,
/// and `owner_id` is `None` there anyway.
///
/// The system's own lines are never mutable: they carry no id to block, and
/// swallowing them would hide region restarts and the viewer's own notices.
#[must_use]
pub fn chat_text_muted(mutes: &MuteList, message: &ChatMessage) -> bool {
    let by_speaker = |id: Uuid| mutes.text_muted(id, &message.from_name);
    match message.source {
        ChatSource::Agent(agent) => by_speaker(agent.uuid()),
        ChatSource::Object(object) => {
            by_speaker(object.uuid())
                || message
                    .owner_id
                    .is_some_and(|owner| mutes.is_muted_aspect(owner, MuteFlags::ALLOW_TEXT_CHAT))
        }
        ChatSource::System | ChatSource::Unknown { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{MuteList, chat_text_muted};
    use crate::types::{
        ChatAudible, ChatMessage, ChatSource, ChatType, Event, ImDialog, InstantMessage, MuteEntry,
        MuteFlags, MuteType,
    };
    use pretty_assertions::assert_eq;
    use sl_types::key::{AgentKey, GroupKey, ObjectKey};
    use sl_types::map::RegionCoordinates;
    use uuid::Uuid;

    /// A mute-list entry blocking `id` under `name` as `mute_type`, with the
    /// given exception bits (`MuteFlags::default()` mutes every aspect).
    fn mute(id: Uuid, name: &str, mute_type: MuteType, flags: u32) -> MuteEntry {
        MuteEntry {
            id,
            name: name.to_owned(),
            mute_type,
            flags: MuteFlags(flags),
        }
    }

    /// A received nearby-chat message from `source`, named `from_name`, with an
    /// optional owner (an object's).
    fn said(from_name: &str, source: ChatSource, owner_id: Option<Uuid>) -> ChatMessage {
        ChatMessage {
            from_name: from_name.to_owned(),
            source,
            owner_id,
            chat_type: ChatType::Normal,
            audible: ChatAudible::Fully,
            position: RegionCoordinates::new(0.0, 0.0, 0.0),
            message: "hello".to_owned(),
        }
    }

    /// The text-chat aspect is muted only for a target actually on the list and
    /// only while that entry does *not* carry the text exception — and a
    /// **by-name** entry (what *Block object by name…* writes) matches a
    /// speaker whose id is nowhere on the list, which is the whole point of
    /// blocking by name.
    #[test]
    fn text_mute_honours_the_exception_bit_and_the_by_name_fallback() {
        let troll = Uuid::from_u128(0x11);
        let quiet = Uuid::from_u128(0x22);
        let spammer = Uuid::from_u128(0x33);
        let mut list = MuteList::new();
        list.replace(vec![
            mute(troll, "Troll Resident", MuteType::Agent, 0),
            // Blocked, but text chat excepted — they may still speak.
            mute(
                quiet,
                "Quiet Resident",
                MuteType::Agent,
                MuteFlags::ALLOW_TEXT_CHAT,
            ),
            mute(Uuid::nil(), "Ad Spammer", MuteType::ByName, 0),
        ]);

        assert!(list.text_muted(troll, "Troll Resident"));
        assert!(
            !list.text_muted(quiet, "Quiet Resident"),
            "an entry carrying the text exception is not text-muted"
        );
        assert!(
            list.text_muted(spammer, "ad spammer"),
            "a by-name entry matches case-insensitively, whatever the speaker's id"
        );
        assert!(
            !list.text_muted(spammer, "Someone Else"),
            "an unblocked speaker stays unblocked"
        );
        assert!(
            !list.text_muted(Uuid::nil(), ""),
            "a nil id with no name matches nothing — not every by-name entry at once"
        );
    }

    /// An object's chat is silenced by a block on the object *or* on its owner,
    /// the same pair of keys a sound from it is silenced by; the system's own
    /// lines are never blocked.
    #[test]
    fn object_chat_is_muted_by_object_or_owner() {
        let owner = Uuid::from_u128(0x44);
        let object = Uuid::from_u128(0x55);
        let other = Uuid::from_u128(0x66);
        let mut owner_blocked = MuteList::new();
        owner_blocked.replace(vec![mute(owner, "Rezzer Resident", MuteType::Agent, 0)]);
        let mut object_blocked = MuteList::new();
        object_blocked.replace(vec![mute(object, "Yapping Cube", MuteType::Object, 0)]);

        let chatted = said(
            "Yapping Cube",
            ChatSource::Object(ObjectKey::from(object)),
            Some(owner),
        );
        assert!(chat_text_muted(&owner_blocked, &chatted), "owner blocked");
        assert!(chat_text_muted(&object_blocked, &chatted), "object blocked");
        assert!(
            !chat_text_muted(
                &owner_blocked,
                &said(
                    "Innocent Cube",
                    ChatSource::Object(ObjectKey::from(other)),
                    Some(other),
                ),
            ),
            "another owner's object is untouched"
        );
        assert!(
            !chat_text_muted(
                &owner_blocked,
                &said("Second Life", ChatSource::System, None)
            ),
            "the system has no id to block and is never swallowed"
        );
    }

    /// The four events that carry something a resident said are all filtered
    /// by a block — nearby chat, a 1:1 IM, a group line and a conference line —
    /// and an event that carries no line of theirs is not this test's business,
    /// so the surfaces that must react to one (an invitation to decline, a
    /// typing notice that would open a tab) still see it.
    #[test]
    fn every_line_a_blocked_resident_says_is_filtered_and_nothing_else_is() {
        let troll = AgentKey::from(Uuid::from_u128(0x88));
        let quiet = AgentKey::from(Uuid::from_u128(0x99));
        let mut list = MuteList::new();
        list.replace(vec![mute(
            troll.uuid(),
            "Troll Resident",
            MuteType::Agent,
            0,
        )]);

        let nearby = |who: AgentKey, name: &str| {
            Event::ChatReceived(Box::new(said(name, ChatSource::Agent(who), None)))
        };
        let im = |who: AgentKey, name: &str| {
            Event::InstantMessageReceived(Box::new(InstantMessage {
                from_agent_id: who,
                from_agent_name: name.to_owned(),
                to_agent_id: AgentKey::from(Uuid::from_u128(1)),
                dialog: ImDialog::Message,
                from_group: false,
                region_id: None,
                position: RegionCoordinates::new(0.0, 0.0, 0.0),
                offline: false,
                timestamp: None,
                id: Uuid::nil(),
                parent_estate_id: 0,
                message: "hello".to_owned(),
                binary_bucket: Vec::new(),
            }))
        };
        let group = |who: AgentKey, name: &str| Event::GroupSessionMessage {
            group_id: GroupKey::from(Uuid::from_u128(0x10)),
            from_agent_id: who,
            from_name: name.to_owned(),
            message: "hello".to_owned(),
        };
        let conference = |who: AgentKey, name: &str| Event::ConferenceSessionMessage {
            session_id: Uuid::from_u128(0x20),
            from_agent_id: who,
            from_name: name.to_owned(),
            message: "hello".to_owned(),
        };

        for said_by_troll in [
            nearby(troll, "Troll Resident"),
            im(troll, "Troll Resident"),
            group(troll, "Troll Resident"),
            conference(troll, "Troll Resident"),
        ] {
            assert!(
                list.text_muted_event(&said_by_troll),
                "a blocked resident's line is filtered: {said_by_troll:?}"
            );
        }
        for said_by_quiet in [
            nearby(quiet, "Quiet Resident"),
            im(quiet, "Quiet Resident"),
            group(quiet, "Quiet Resident"),
            conference(quiet, "Quiet Resident"),
        ] {
            assert!(
                !list.text_muted_event(&said_by_quiet),
                "an unblocked resident is untouched: {said_by_quiet:?}"
            );
        }

        // Their typing notice carries no line: the conversation surface decides
        // what to do with it (it suppresses it, because it would open a tab),
        // and the transcript has nothing to write either way.
        assert!(!list.text_muted_event(&Event::ChatTyping {
            from_name: "Troll Resident".to_owned(),
            source_id: troll.uuid(),
            typing: true,
        }));
    }

    /// A locally-issued mute lands without waiting for a re-request, an unmute
    /// takes it back off, and a re-mute of the same target **replaces** the
    /// entry rather than duplicating it (that is how a flag edit lands).
    #[test]
    fn local_mutes_and_unmutes_move_the_list() {
        let troll = Uuid::from_u128(0x77);
        let mut list = MuteList::new();
        list.note_mute(mute(troll, "Troll Resident", MuteType::Agent, 0));
        assert!(list.is_muted(troll));
        assert!(list.text_muted(troll, "Troll Resident"));

        // A flag edit re-sends the whole entry: one entry, now text-excepted.
        list.note_mute(mute(
            troll,
            "Troll Resident",
            MuteType::Agent,
            MuteFlags::ALLOW_TEXT_CHAT,
        ));
        assert_eq!(list.entries().len(), 1);
        assert!(list.is_muted(troll), "still blocked, just not for text");
        assert!(!list.text_muted(troll, "Troll Resident"));

        list.note_unmute(troll, "Troll Resident");
        assert!(list.entries().is_empty());
        assert!(!list.is_muted(troll));
    }
}
