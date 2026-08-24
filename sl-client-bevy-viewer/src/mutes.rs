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
//!
//! # One guarded way in
//!
//! Every Block affordance in the viewer writes a [`RequestBlock`] rather than a
//! `Command::Mute`; [`apply_block_requests`] runs the reference's
//! `LLMuteList::add` guards ([`check_block`]) and only then puts the entry on
//! the wire. That is why the checks live here and not in the block-list UI —
//! a command already written to the outgoing stream cannot be un-sent, so the
//! refusal has to happen before the surface commits to it.
//!
//! # Entries, not just ids
//!
//! The model keeps the **whole** [`MuteEntry`] (name, type and the per-aspect
//! [`MuteFlags`] exceptions), because the block-list surface
//! ([`crate::blocked`]) lists and edits them; a derived [`HashSet`] of muted
//! ids keeps the hot per-frame `is_muted` query (name tags, world sounds) a
//! single hash lookup, as it was when that was all the model held.
//!
//! # Matching an entry
//!
//! An entry is identified by its id when it has one, and by its (case-folded)
//! name when it does not — a [`MuteType::ByName`] mute carries a nil id, so
//! several such entries would otherwise collapse onto one key. The reference
//! keys its set on the id / type / name triple, but a caller here often knows
//! only a partial name (an object mute recorded before the properties reply
//! landed), so matching a non-nil id ignores the name deliberately.

use bevy::prelude::*;

use sl_client_bevy::{
    Command, MuteEntry, MuteFlags, MuteType, SlCommand, SlEvent, SlIdentity, SlSessionEvent, Uuid,
};

use crate::notifications::ShowNotification;
use crate::world_api::{MUTE_LIST_LIMIT, MuteModel, RequestBlock};

/// Request the mute list once the session is up (the login handshake has
/// produced an agent id).
pub(crate) fn request_mute_list(
    identity: Res<SlIdentity>,
    mut model: ResMut<MuteModel>,
    mut commands: MessageWriter<SlCommand>,
) {
    if identity.agent_id.is_none() || !model.claim_request() {
        return;
    }
    commands.write(SlCommand(Command::RequestMuteList));
}

/// Fold a received mute list into the model (`MuteList` replaces the list;
/// `MuteListUnchanged` means the cached copy the request named is current —
/// nothing to do, and the locally-noted entries stay).
pub(crate) fn ingest_mute_list(mut events: MessageReader<SlEvent>, mut model: ResMut<MuteModel>) {
    for event in events.read() {
        if let SlSessionEvent::MuteList(entries) = &event.0 {
            model.replace(entries.clone());
        }
    }
}

/// Mirror locally-issued mutes/unmutes into the model by watching the outgoing
/// command stream (every mute menu writes an [`SlCommand`], so no call site
/// needs to know this model exists).
pub(crate) fn note_local_mutes(
    mut outgoing: MessageReader<SlCommand>,
    mut model: ResMut<MuteModel>,
) {
    for command in outgoing.read() {
        match &command.0 {
            Command::Mute {
                id,
                name,
                mute_type,
                flags,
            } => {
                model.note_mute(MuteEntry {
                    id: *id,
                    name: name.clone(),
                    mute_type: *mute_type,
                    flags: *flags,
                });
            }
            Command::Unmute { id, name } => {
                model.note_unmute(*id, name);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// The guarded block request — the one way into the mute list.
// ---------------------------------------------------------------------------

/// Why a block was refused, mirroring the `LLMuteList::add` early-outs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockRefusal {
    /// The list already holds [`MUTE_LIST_LIMIT`] entries.
    Full,
    /// The target is a Linden and the block would silence their text chat.
    Linden,
    /// A by-name entry with that name already exists.
    Duplicate,
    /// The target is the agent itself.
    Own,
    /// A by-name request with an empty name or a non-nil id.
    Malformed,
}

impl BlockRefusal {
    /// The catalogue template this refusal raises, or `None` for the two the
    /// reference only logs (blocking yourself, a malformed by-name request) —
    /// neither is reachable from a UI affordance, so a toast would be noise.
    const fn template(self) -> Option<&'static str> {
        match self {
            Self::Full => Some("MuteLimitReached"),
            Self::Linden => Some("MuteLinden"),
            Self::Duplicate => Some("MuteByNameFailed"),
            Self::Own | Self::Malformed => None,
        }
    }
}

/// Whether `name` is a Linden account (the reference refuses to silence one's
/// text chat). The reference tests the **last** name, so a resident merely
/// *called* "Linden Something" is still blockable.
fn is_linden(name: &str) -> bool {
    name.rsplit(' ')
        .next()
        .is_some_and(|last| last.eq_ignore_ascii_case("Linden"))
}

/// Run the reference's `LLMuteList::add` guards over `request` — `Ok(())` when
/// the block may go on the wire.
///
/// One deliberate divergence: the list-full check is skipped when the target is
/// **already** on the list, because such a request is an *update* (the block
/// list's aspect toggles re-send the whole entry) and adds no row. The
/// reference refuses those too once the list is full, which would silently wedge
/// the toggles at exactly the limit.
pub(crate) fn check_block(
    model: &MuteModel,
    own_agent: Option<Uuid>,
    request: &RequestBlock,
) -> Result<(), BlockRefusal> {
    if matches!(request.mute_type, MuteType::Agent) {
        if own_agent == Some(request.id) {
            return Err(BlockRefusal::Own);
        }
        if is_linden(&request.name) && !request.flags.contains(MuteFlags::ALLOW_TEXT_CHAT) {
            return Err(BlockRefusal::Linden);
        }
    }
    if matches!(request.mute_type, MuteType::ByName) {
        if request.name.trim().is_empty() || !request.id.is_nil() {
            return Err(BlockRefusal::Malformed);
        }
        if model.has_by_name(&request.name) {
            return Err(BlockRefusal::Duplicate);
        }
    }
    if model.entry(request.id, &request.name).is_none() && model.is_full() {
        return Err(BlockRefusal::Full);
    }
    Ok(())
}

/// Turn each [`RequestBlock`] into an `UpdateMuteListEntry` — or, when a guard
/// refuses it, into the matching notification.
pub(crate) fn apply_block_requests(
    mut requests: MessageReader<RequestBlock>,
    model: Res<MuteModel>,
    identity: Res<SlIdentity>,
    mut commands: MessageWriter<SlCommand>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let own = identity.agent_id.map(|agent| agent.uuid());
    for request in requests.read() {
        match check_block(&model, own, request) {
            Ok(()) => {
                commands.write(SlCommand(Command::Mute {
                    id: request.id,
                    name: request.name.clone(),
                    mute_type: request.mute_type,
                    flags: request.flags,
                }));
            }
            Err(refusal) => {
                if let Some(template) = refusal.template() {
                    let mut notification = ShowNotification::new(template);
                    if refusal == BlockRefusal::Full {
                        notification = notification.arg("MUTE_LIMIT", MUTE_LIST_LIMIT.to_string());
                    }
                    notifications.write(notification);
                } else {
                    debug!("block of {:?} refused: {refusal:?}", request.name);
                }
            }
        }
    }
}

/// Whether `mute_type` names an entry whose per-aspect flags are meaningful.
/// The reference offers the text / voice / particles / object-sound toggles
/// only for a resident (`AGENT`) mute; every other kind is all-or-nothing.
pub(crate) const fn flags_apply(mute_type: MuteType) -> bool {
    matches!(mute_type, MuteType::Agent)
}

#[cfg(test)]
mod tests {
    use super::{BlockRefusal, MUTE_LIST_LIMIT, MuteModel, RequestBlock, check_block, is_linden};
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{MuteEntry, MuteFlags, MuteType, Uuid};

    /// A resident mute of `id` with `flags`.
    fn agent_entry(id: Uuid, name: &str, flags: u32) -> MuteEntry {
        MuteEntry {
            id,
            name: name.to_owned(),
            mute_type: MuteType::Agent,
            flags: MuteFlags(flags),
        }
    }

    /// Local notes flip membership immediately; a list ingest replaces it.
    #[test]
    fn notes_and_replacement() {
        let mut model = MuteModel::default();
        let troll = Uuid::from_u128(0xBAD);
        assert!(!model.is_muted(troll));
        model.note_mute(agent_entry(troll, "Troll Resident", 0));
        assert!(model.is_muted(troll));
        model.note_unmute(troll, "Troll Resident");
        assert!(!model.is_muted(troll));
    }

    /// A second mute of the same id replaces the entry (that is how a flag
    /// edit lands) rather than appending a duplicate row.
    #[test]
    fn re_mute_replaces_the_entry() {
        let mut model = MuteModel::default();
        let troll = Uuid::from_u128(0xBAD);
        model.note_mute(agent_entry(troll, "Troll Resident", 0));
        model.note_mute(agent_entry(
            troll,
            "Troll Resident",
            MuteFlags::ALLOW_VOICE_CHAT,
        ));
        assert_eq!(model.entries().len(), 1);
        assert_eq!(
            model.entries().first().map(|entry| entry.flags),
            Some(MuteFlags(MuteFlags::ALLOW_VOICE_CHAT))
        );
    }

    /// A set exception bit means that aspect is *not* muted; the id still
    /// counts as muted overall.
    #[test]
    fn aspect_exceptions() {
        let mut model = MuteModel::default();
        let troll = Uuid::from_u128(0xBAD);
        model.note_mute(agent_entry(troll, "Troll", MuteFlags::ALLOW_VOICE_CHAT));
        assert!(model.is_muted(troll));
        assert!(!model.is_muted_aspect(troll, MuteFlags::ALLOW_VOICE_CHAT));
        assert!(model.is_muted_aspect(troll, MuteFlags::ALLOW_TEXT_CHAT));
    }

    /// By-name entries share a nil id, so they are told apart by name — and a
    /// nil id never registers as a muted id.
    #[test]
    fn by_name_entries_are_keyed_by_name() {
        let mut model = MuteModel::default();
        for name in ["Spam Vendor", "Loud Sign"] {
            model.note_mute(MuteEntry {
                id: Uuid::nil(),
                name: name.to_owned(),
                mute_type: MuteType::ByName,
                flags: MuteFlags(0),
            });
        }
        assert_eq!(model.entries().len(), 2);
        assert!(model.has_by_name("spam vendor"));
        assert!(!model.is_muted(Uuid::nil()));
        model.note_unmute(Uuid::nil(), "Spam Vendor");
        assert_eq!(model.entries().len(), 1);
        assert!(!model.has_by_name("Spam Vendor"));
    }

    /// The revision advances on every change, so a view can rebuild off it.
    #[test]
    fn revision_advances() {
        let mut model = MuteModel::default();
        let before = model.revision();
        model.note_mute(agent_entry(Uuid::from_u128(1), "One", 0));
        assert_ne!(model.revision(), before);
    }

    /// Only a trailing "Linden" surname counts.
    #[test]
    fn linden_detection() {
        assert!(is_linden("Torley Linden"));
        assert!(is_linden("torley linden"));
        assert!(!is_linden("Linden Resident"));
        assert!(!is_linden("Someone Else"));
    }

    /// The `LLMuteList::add` guards: self, Linden (text chat only), by-name
    /// shape and duplicates, and the list limit.
    #[test]
    fn add_guards() {
        let mut model = MuteModel::default();
        let own = Uuid::from_u128(0xAAA);
        let request = |id: u128, name: &str, mute_type| {
            RequestBlock::new(Uuid::from_u128(id), name, mute_type)
        };

        assert_eq!(
            check_block(
                &model,
                Some(own),
                &request(1, "Some Resident", MuteType::Agent)
            ),
            Ok(())
        );
        assert_eq!(
            check_block(
                &model,
                Some(own),
                &request(0xAAA, "Me Myself", MuteType::Agent)
            ),
            Err(BlockRefusal::Own)
        );
        assert_eq!(
            check_block(
                &model,
                Some(own),
                &request(2, "Torley Linden", MuteType::Agent)
            ),
            Err(BlockRefusal::Linden)
        );
        // Leaving a Linden's text chat alone is allowed (the reference gates the
        // refusal on the text-chat aspect), as is blocking a Linden-named object.
        assert_eq!(
            check_block(
                &model,
                Some(own),
                &request(2, "Torley Linden", MuteType::Agent)
                    .with_flags(MuteFlags(MuteFlags::ALLOW_TEXT_CHAT))
            ),
            Ok(())
        );
        assert_eq!(
            check_block(
                &model,
                Some(own),
                &request(2, "Torley Linden", MuteType::Object)
            ),
            Ok(())
        );

        // A by-name request must carry a nil id and a non-blank name.
        assert_eq!(
            check_block(
                &model,
                Some(own),
                &request(3, "Spam Vendor", MuteType::ByName)
            ),
            Err(BlockRefusal::Malformed)
        );
        let by_name = RequestBlock::new(Uuid::nil(), "Spam Vendor", MuteType::ByName);
        assert_eq!(check_block(&model, Some(own), &by_name), Ok(()));

        // An id-keyed entry of the same name does not make it a duplicate; a
        // by-name one does.
        model.note_mute(MuteEntry {
            id: Uuid::from_u128(7),
            name: "Spam Vendor".to_owned(),
            mute_type: MuteType::Object,
            flags: MuteFlags(0),
        });
        assert_eq!(check_block(&model, Some(own), &by_name), Ok(()));
        model.note_mute(MuteEntry {
            id: Uuid::nil(),
            name: "Spam Vendor".to_owned(),
            mute_type: MuteType::ByName,
            flags: MuteFlags(0),
        });
        assert_eq!(
            check_block(&model, Some(own), &by_name),
            Err(BlockRefusal::Duplicate)
        );

        // A full list refuses a new target but still lets an existing entry be
        // updated (that is how the aspect toggles re-send).
        for index in 0..MUTE_LIST_LIMIT {
            let id = u128::try_from(index).unwrap_or(0).wrapping_add(1000);
            model.note_mute(agent_entry(Uuid::from_u128(id), "Filler", 0));
        }
        assert_eq!(
            check_block(&model, Some(own), &request(4, "Anyone", MuteType::Agent)),
            Err(BlockRefusal::Full)
        );
        assert_eq!(
            check_block(&model, Some(own), &request(1000, "Filler", MuteType::Agent)),
            Ok(())
        );
    }

    /// The list reports full exactly at the reference's limit.
    #[test]
    fn fullness_at_the_limit() {
        let mut model = MuteModel::default();
        assert!(!model.is_full());
        for index in 0..MUTE_LIST_LIMIT {
            let id = Uuid::from_u128(u128::try_from(index).unwrap_or(0).wrapping_add(1));
            model.note_mute(agent_entry(id, "Filler", 0));
        }
        assert!(model.is_full());
    }
}
