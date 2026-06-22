//! Chat and instant messaging value types.

use super::AssetType;
use sl_types::key::AgentKey;
use uuid::Uuid;

/// The kind of a chat message, from the `Type`/`ChatType` byte shared by
/// `ChatFromViewer` (outgoing) and `ChatFromSimulator` (incoming).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChatType {
    /// Whisper: a reduced (10 m) range.
    Whisper,
    /// Normal local say: the default (20 m) range.
    Normal,
    /// Shout: an extended (100 m) range.
    Shout,
    /// "Start typing" animation trigger (no text).
    StartTyping,
    /// "Stop typing" animation trigger (no text).
    StopTyping,
    /// A debug-channel message (script errors; channel `2147483647`).
    DebugChannel,
    /// A region-wide message.
    Region,
    /// A message from an object to its owner.
    Owner,
    /// A directed message to a single agent (`llRegionSayTo`).
    Direct,
    /// An unrecognised type byte, preserved verbatim.
    Unknown(u8),
}

impl ChatType {
    /// Classifies a `Type`/`ChatType` byte.
    #[must_use]
    pub const fn from_u8(byte: u8) -> Self {
        match byte {
            0 => Self::Whisper,
            1 => Self::Normal,
            2 => Self::Shout,
            4 => Self::StartTyping,
            5 => Self::StopTyping,
            6 => Self::DebugChannel,
            7 => Self::Region,
            8 => Self::Owner,
            9 => Self::Direct,
            other => Self::Unknown(other),
        }
    }

    /// The wire byte for this chat type.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Whisper => 0,
            Self::Normal => 1,
            Self::Shout => 2,
            Self::StartTyping => 4,
            Self::StopTyping => 5,
            Self::DebugChannel => 6,
            Self::Region => 7,
            Self::Owner => 8,
            Self::Direct => 9,
            Self::Unknown(other) => other,
        }
    }
}

/// What kind of source produced a chat message, from the `SourceType` byte of
/// `ChatFromSimulator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChatSourceType {
    /// The system / region (no avatar or object).
    System,
    /// An avatar.
    Agent,
    /// An in-world object.
    Object,
    /// An unrecognised source-type byte, preserved verbatim.
    Unknown(u8),
}

impl ChatSourceType {
    /// Classifies a `SourceType` byte.
    #[must_use]
    pub const fn from_u8(byte: u8) -> Self {
        match byte {
            0 => Self::System,
            1 => Self::Agent,
            2 => Self::Object,
            other => Self::Unknown(other),
        }
    }
}

/// Whether a chat message was audible at the listener, from the `Audible` byte
/// of `ChatFromSimulator` (a signed value: `-1`/`255` means not audible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChatAudible {
    /// Not audible (out of range); the message text may be elided.
    Not,
    /// Barely audible (at the edge of range).
    Barely,
    /// Fully audible.
    Fully,
    /// An unrecognised audibility byte, preserved verbatim.
    Unknown(u8),
}

impl ChatAudible {
    /// Classifies an `Audible` byte (`255`/`-1` = not, `0` = barely, `1` = fully).
    #[must_use]
    pub const fn from_u8(byte: u8) -> Self {
        match byte {
            255 => Self::Not,
            0 => Self::Barely,
            1 => Self::Fully,
            other => Self::Unknown(other),
        }
    }
}

/// A chat message received from the simulator, parsed from `ChatFromSimulator`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    /// The display name of the speaker (avatar legacy name or object name).
    pub from_name: String,
    /// The speaker's id (agent id or object id), or nil for the system.
    pub source_id: Uuid,
    /// For an object speaker, its owner's agent id; nil otherwise.
    pub owner_id: Uuid,
    /// What kind of source produced the message.
    pub source_type: ChatSourceType,
    /// The chat type (whisper / normal / shout / …).
    pub chat_type: ChatType,
    /// Whether the message was audible at the listener.
    pub audible: ChatAudible,
    /// The speaker's region-local position, in metres.
    pub position: (f32, f32, f32),
    /// The message text (UTF-8, with any trailing NUL padding removed).
    pub message: String,
}

/// The kind of an instant message, from the `Dialog` byte of
/// `ImprovedInstantMessage` (the `EInstantMessage` enum in the protocol). Only
/// the commonly handled dialogs are named; the rest are preserved verbatim via
/// [`ImDialog::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImDialog {
    /// An ordinary 1:1 instant message (`IM_NOTHING_SPECIAL`).
    Message,
    /// A modal message box from an object (`IM_MESSAGEBOX`).
    MessageBox,
    /// A group invitation (`IM_GROUP_INVITATION`).
    GroupInvitation,
    /// An inventory item offered to the agent (`IM_INVENTORY_OFFERED`).
    InventoryOffered,
    /// An inventory offer was accepted (`IM_INVENTORY_ACCEPTED`).
    InventoryAccepted,
    /// An inventory offer was declined (`IM_INVENTORY_DECLINED`).
    InventoryDeclined,
    /// An inventory item offered by a task/object (`IM_TASK_INVENTORY_OFFERED`).
    TaskInventoryOffered,
    /// A participant was added to a group/conference session
    /// (`IM_SESSION_INVITE` / OpenMetaverse `SessionAdd`).
    SessionAdd,
    /// An offline participant was added to a session (`IM_SESSION_P2P_INVITE` /
    /// OpenMetaverse `SessionOfflineAdd`).
    SessionOfflineAdd,
    /// A request to start a group IM session (`IM_SESSION_GROUP_START`); the
    /// session id is the group id.
    SessionGroupStart,
    /// A request to start an ad-hoc conference IM session
    /// (`IM_SESSION_CONFERENCE_START`).
    SessionConferenceStart,
    /// A message within a group or conference session (`IM_SESSION_SEND`).
    SessionSend,
    /// A participant left / was dropped from a session (`IM_SESSION_LEAVE` /
    /// OpenMetaverse `SessionDrop`).
    SessionLeave,
    /// A message from an in-world object/task (`IM_FROM_TASK`).
    FromTask,
    /// A "do not disturb" auto-response (`IM_DO_NOT_DISTURB_AUTO_RESPONSE`).
    DoNotDisturbAutoResponse,
    /// A teleport offer / lure (`IM_LURE_USER`).
    LureUser,
    /// A teleport offer was accepted (`IM_LURE_ACCEPTED`).
    LureAccepted,
    /// A teleport offer was declined (`IM_LURE_DECLINED`).
    LureDeclined,
    /// A request to be teleported to the sender (`IM_TELEPORT_REQUEST`).
    TeleportRequest,
    /// A request to open a URL (`IM_GOTO_URL`).
    GotoUrl,
    /// A group notice (`IM_GROUP_NOTICE`).
    GroupNotice,
    /// A friendship offer (`IM_FRIENDSHIP_OFFERED`).
    FriendshipOffered,
    /// A friendship offer was accepted (`IM_FRIENDSHIP_ACCEPTED`).
    FriendshipAccepted,
    /// The correspondent started typing (`IM_TYPING_START`).
    TypingStart,
    /// The correspondent stopped typing (`IM_TYPING_STOP`).
    TypingStop,
    /// An unrecognised dialog byte, preserved verbatim.
    Unknown(u8),
}

impl ImDialog {
    /// Classifies a `Dialog` byte.
    #[must_use]
    pub const fn from_u8(byte: u8) -> Self {
        match byte {
            0 => Self::Message,
            1 => Self::MessageBox,
            3 => Self::GroupInvitation,
            4 => Self::InventoryOffered,
            5 => Self::InventoryAccepted,
            6 => Self::InventoryDeclined,
            9 => Self::TaskInventoryOffered,
            13 => Self::SessionAdd,
            14 => Self::SessionOfflineAdd,
            15 => Self::SessionGroupStart,
            16 => Self::SessionConferenceStart,
            17 => Self::SessionSend,
            18 => Self::SessionLeave,
            19 => Self::FromTask,
            20 => Self::DoNotDisturbAutoResponse,
            22 => Self::LureUser,
            23 => Self::LureAccepted,
            24 => Self::LureDeclined,
            26 => Self::TeleportRequest,
            28 => Self::GotoUrl,
            32 => Self::GroupNotice,
            38 => Self::FriendshipOffered,
            39 => Self::FriendshipAccepted,
            41 => Self::TypingStart,
            42 => Self::TypingStop,
            other => Self::Unknown(other),
        }
    }

    /// The wire byte for this dialog.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Message => 0,
            Self::MessageBox => 1,
            Self::GroupInvitation => 3,
            Self::InventoryOffered => 4,
            Self::InventoryAccepted => 5,
            Self::InventoryDeclined => 6,
            Self::TaskInventoryOffered => 9,
            Self::SessionAdd => 13,
            Self::SessionOfflineAdd => 14,
            Self::SessionGroupStart => 15,
            Self::SessionConferenceStart => 16,
            Self::SessionSend => 17,
            Self::SessionLeave => 18,
            Self::FromTask => 19,
            Self::DoNotDisturbAutoResponse => 20,
            Self::LureUser => 22,
            Self::LureAccepted => 23,
            Self::LureDeclined => 24,
            Self::TeleportRequest => 26,
            Self::GotoUrl => 28,
            Self::GroupNotice => 32,
            Self::FriendshipOffered => 38,
            Self::FriendshipAccepted => 39,
            Self::TypingStart => 41,
            Self::TypingStop => 42,
            Self::Unknown(other) => other,
        }
    }
}

/// An instant message received from the simulator, parsed from
/// `ImprovedInstantMessage`. Many fields are dialog-dependent (notably
/// [`InstantMessage::id`] and [`InstantMessage::binary_bucket`]); see
/// [`ImDialog`].
#[derive(Debug, Clone, PartialEq)]
pub struct InstantMessage {
    /// The sender's agent id.
    pub from_agent_id: AgentKey,
    /// The sender's display name (with any trailing NUL padding removed).
    pub from_agent_name: String,
    /// The recipient's agent id (this agent for a direct IM, or a group id).
    pub to_agent_id: AgentKey,
    /// The dialog (sub-type) of the message.
    pub dialog: ImDialog,
    /// Whether the message came from a group (rather than an agent).
    pub from_group: bool,
    /// The source region's id (nil if not provided).
    pub region_id: Uuid,
    /// The sender's region-local position, in metres.
    pub position: (f32, f32, f32),
    /// Whether the message was stored-and-forwarded while the agent was offline.
    pub offline: bool,
    /// The sender's timestamp (`0` when unset; the simulator often fills it).
    pub timestamp: u32,
    /// A dialog-dependent id: the IM session id for chats, or a transaction id
    /// for offers.
    pub id: Uuid,
    /// The parent estate id of the source.
    pub parent_estate_id: u32,
    /// The message text (UTF-8, with any trailing NUL padding removed).
    pub message: String,
    /// Dialog-dependent binary payload (e.g. an inventory offer's asset type and
    /// item id, a group invite's role and fee). Empty for an ordinary IM.
    pub binary_bucket: Vec<u8>,
}

impl InstantMessage {
    /// Decodes the inventory-offer descriptor from this message's binary bucket,
    /// for an [`ImDialog::InventoryOffered`] or [`ImDialog::TaskInventoryOffered`]
    /// message. The bucket is `[asset-type byte] ++ [16-byte item/folder id]`
    /// (a folder offer leads with [`AssetType::Folder`]); only the first entry is
    /// returned. Returns `None` for any other dialog or a malformed bucket.
    #[must_use]
    pub fn inventory_offer(&self) -> Option<InventoryOffer> {
        if !matches!(
            self.dialog,
            ImDialog::InventoryOffered | ImDialog::TaskInventoryOffered
        ) {
            return None;
        }
        let type_byte = *self.binary_bucket.first()?;
        let id_bytes: [u8; 16] = self.binary_bucket.get(1..17)?.try_into().ok()?;
        Some(InventoryOffer {
            asset_type: AssetType::from_code(i32::from(type_byte)),
            item_id: Uuid::from_bytes(id_bytes),
            transaction_id: self.id,
            from_agent_id: self.from_agent_id,
            from_task: matches!(self.dialog, ImDialog::TaskInventoryOffered),
        })
    }
}

/// An inventory offer received over IM, decoded from the binary bucket of an
/// [`ImDialog::InventoryOffered`] / [`ImDialog::TaskInventoryOffered`]
/// [`InstantMessage`] (see [`InstantMessage::inventory_offer`]). Reply with
/// [`Session::accept_inventory_offer`](crate::Session::accept_inventory_offer)
/// or [`Session::decline_inventory_offer`](crate::Session::decline_inventory_offer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryOffer {
    /// The offered asset's class ([`AssetType::Folder`] for a whole folder).
    pub asset_type: AssetType,
    /// The offered item's (or folder's) id.
    pub item_id: Uuid,
    /// The offer's transaction id (the IM's `id`), echoed back when replying.
    pub transaction_id: Uuid,
    /// The agent (or, for a task offer, the object owner) that made the offer.
    pub from_agent_id: AgentKey,
    /// Whether the offer came from an in-world object/task rather than an agent.
    pub from_task: bool,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::AgentKey;
    use uuid::Uuid;

    use super::{AssetType, ImDialog, InstantMessage};

    /// An [`AgentKey`] is a transparent wrapper over its [`Uuid`]: wrapping a raw
    /// id and unwrapping it again yields the identical bytes, so the on-wire
    /// representation is unchanged by the newtype.
    #[test]
    fn agent_key_round_trips_uuid_bit_identically() {
        for raw in [
            Uuid::nil(),
            Uuid::from_u128(1),
            Uuid::from_u128(0xdead_beef_dead_beef_dead_beef_dead_beef),
        ] {
            assert_eq!(AgentKey::from(raw).uuid(), raw);
        }
    }

    /// The sender id carried on an [`InstantMessage`] survives the inventory-offer
    /// extraction path unchanged — the `from_agent_id` typed as [`AgentKey`] is
    /// copied through to the [`InventoryOffer`] with byte-identical contents.
    #[test]
    fn instant_message_from_agent_id_survives_inventory_offer() -> Result<(), String> {
        let sender = Uuid::from_u128(0xa11);
        let item = Uuid::from_u128(0xbb22);
        // The bucket is `[asset-type byte] ++ [16-byte item id]`; 7 is `Notecard`.
        let mut bucket = vec![7_u8];
        bucket.extend_from_slice(item.as_bytes());
        let im = InstantMessage {
            from_agent_id: AgentKey::from(sender),
            from_agent_name: "Giver Resident".to_owned(),
            to_agent_id: AgentKey::from(Uuid::from_u128(0xa12)),
            dialog: ImDialog::InventoryOffered,
            from_group: false,
            region_id: Uuid::nil(),
            position: (0.0, 0.0, 0.0),
            offline: false,
            timestamp: 0,
            id: Uuid::from_u128(0xcc33),
            parent_estate_id: 0,
            message: "here you go".to_owned(),
            binary_bucket: bucket,
        };
        let offer = im
            .inventory_offer()
            .ok_or_else(|| "expected a valid inventory offer".to_owned())?;
        assert_eq!(offer.from_agent_id, im.from_agent_id);
        assert_eq!(offer.from_agent_id.uuid(), sender);
        assert_eq!(offer.item_id, item);
        assert_eq!(offer.asset_type, AssetType::Notecard);
        Ok(())
    }
}
