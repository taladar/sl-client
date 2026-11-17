//! Splitting a caller's list of blocks across as many messages as it takes.
//!
//! A `Variable` block's count is one byte on the wire, so one message carries
//! at most [`MAX_VARIABLE_BLOCKS`] of it, and a datagram should stay inside the
//! reference's [`MTU_BYTES`]. A list longer than either — a box-selected build,
//! a large linkset, a region's worth of kills — is not one message but several,
//! each with the same header blocks: the reference viewer fills a message until
//! `isSendFull` says it is full (more than `MTUBYTES` written, or `MAX_BLOCKS`
//! of the block) and starts another (`LLSelectMgr::sendListToRegions`).
//!
//! Both directions use it: the client [`Circuit`](crate::session) senders and
//! the simulator's [`SimSession`](crate::SimSession) senders.

use sl_wire::WireError;
use sl_wire::messages::{
    AnyMessage, RemoveInventoryObjects, RemoveInventoryObjectsAgentDataBlock,
    RemoveInventoryObjectsFolderDataBlock, RemoveInventoryObjectsItemDataBlock,
};
use uuid::Uuid;

/// The most entries one message's variable block list can carry: its count is
/// a single byte on the wire (the reference's `MAX_BLOCKS`).
pub(crate) const MAX_VARIABLE_BLOCKS: usize = 255;

/// The most bytes of encoded message (id and body, before zero coding) one
/// datagram is filled to: the reference's `MTUBYTES`. The reference viewer
/// refuses outright a datagram that decodes past its 8 KiB `NET_BUFFER_SIZE`,
/// and fills its own messages only to this, so neither side sends one past it
/// unless a single block is larger on its own.
pub(crate) const MTU_BYTES: usize = 1200;

/// How many bytes `message` encodes to: its id and body, before zero coding.
pub(crate) fn encoded_len(message: &AnyMessage) -> Result<usize, WireError> {
    let mut writer = sl_wire::Writer::new();
    message.id().encode(&mut writer)?;
    message.encode_body(&mut writer)?;
    Ok(writer.into_bytes().len())
}

/// `blocks` split into the messages `build` makes of them, in order.
///
/// Each message holds as many blocks as fit both `max_blocks` and
/// [`MTU_BYTES`] — `build` with no blocks gives the size of everything else in
/// the message, so header blocks repeated in every message (and a linkset root
/// `build` puts first in each) are counted. A block that does not fit even
/// alone gets a message to itself, which is what the reference does. No blocks
/// make no messages: a caller for which an empty list still means something
/// sends that message itself.
///
/// `max_blocks` is [`MAX_VARIABLE_BLOCKS`] or less; a larger value is held to
/// it, since no message can state more.
pub(crate) fn fill_messages<B: Clone>(
    blocks: Vec<B>,
    max_blocks: usize,
    build: impl Fn(Vec<B>) -> AnyMessage,
) -> Result<Vec<AnyMessage>, WireError> {
    let max_blocks = max_blocks.clamp(1, MAX_VARIABLE_BLOCKS);
    let header = encoded_len(&build(Vec::new()))?;
    let mut messages = Vec::new();
    let mut batch = Vec::new();
    let mut batch_len = header;
    for block in blocks {
        let block_len = encoded_len(&build(vec![block.clone()]))?.saturating_sub(header);
        if !batch.is_empty()
            && (batch.len() >= max_blocks || batch_len.saturating_add(block_len) > MTU_BYTES)
        {
            messages.push(build(core::mem::take(&mut batch)));
            batch_len = header;
        }
        batch.push(block);
        batch_len = batch_len.saturating_add(block_len);
    }
    if !batch.is_empty() {
        messages.push(build(batch));
    }
    Ok(messages)
}

/// [`fill_messages`] for a message whose empty list still says something —
/// "every object of this type" in a parcel selection, "no results" in a
/// directory reply — so no blocks make one message with none, where
/// [`fill_messages`] would send nothing at all.
pub(crate) fn fill_messages_or_one_empty<B: Clone>(
    blocks: Vec<B>,
    max_blocks: usize,
    build: impl Fn(Vec<B>) -> AnyMessage,
) -> Result<Vec<AnyMessage>, WireError> {
    if blocks.is_empty() {
        return Ok(vec![build(Vec::new())]);
    }
    fill_messages(blocks, max_blocks, build)
}

/// One entry of a `RemoveInventoryObjects`: the message carries two lists, and
/// [`remove_inventory_objects`] splits them as one.
#[derive(Debug, Clone, Copy)]
enum RemovedInventoryObject {
    /// A folder, for the `FolderData` list.
    Folder(Uuid),
    /// An item, for the `ItemData` list.
    Item(Uuid),
}

/// The `RemoveInventoryObjects` messages removing `folders` and `items`, from
/// `agent_id` in `session_id`, in as many messages as they take.
///
/// The message has two lists, folders and items, each with its own count byte;
/// the split counts them together against [`MAX_VARIABLE_BLOCKS`] and the MTU,
/// which keeps both inside their limit. Folders go before items, as the
/// reference's batch removal packs them. Nothing to remove makes no messages.
pub(crate) fn remove_inventory_objects(
    agent_id: Uuid,
    session_id: Uuid,
    folders: impl IntoIterator<Item = Uuid>,
    items: impl IntoIterator<Item = Uuid>,
) -> Result<Vec<AnyMessage>, WireError> {
    let entries: Vec<_> = folders
        .into_iter()
        .map(RemovedInventoryObject::Folder)
        .chain(items.into_iter().map(RemovedInventoryObject::Item))
        .collect();
    fill_messages(entries, MAX_VARIABLE_BLOCKS, |entries| {
        let mut folder_data = Vec::new();
        let mut item_data = Vec::new();
        for entry in entries {
            match entry {
                RemovedInventoryObject::Folder(folder_id) => {
                    folder_data.push(RemoveInventoryObjectsFolderDataBlock { folder_id });
                }
                RemovedInventoryObject::Item(item_id) => {
                    item_data.push(RemoveInventoryObjectsItemDataBlock { item_id });
                }
            }
        }
        AnyMessage::RemoveInventoryObjects(RemoveInventoryObjects {
            agent_data: RemoveInventoryObjectsAgentDataBlock {
                agent_id,
                session_id,
            },
            folder_data,
            item_data,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_VARIABLE_BLOCKS, MTU_BYTES, encoded_len, fill_messages, fill_messages_or_one_empty,
        remove_inventory_objects,
    };
    use pretty_assertions::assert_eq;
    use sl_wire::messages::{
        AnyMessage, ObjectName, ObjectNameAgentDataBlock, ObjectNameObjectDataBlock, ObjectSelect,
        ObjectSelectAgentDataBlock, ObjectSelectObjectDataBlock,
    };
    use uuid::Uuid;

    /// An `ObjectSelect` of `ids`.
    fn select(ids: Vec<u32>) -> AnyMessage {
        AnyMessage::ObjectSelect(ObjectSelect {
            agent_data: ObjectSelectAgentDataBlock {
                agent_id: Uuid::nil(),
                session_id: Uuid::nil(),
            },
            object_data: ids
                .into_iter()
                .map(|object_local_id| ObjectSelectObjectDataBlock { object_local_id })
                .collect(),
        })
    }

    /// The ids each `ObjectSelect` of `messages` carries.
    fn selected(messages: &[AnyMessage]) -> Vec<Vec<u32>> {
        messages
            .iter()
            .map(|message| match message {
                AnyMessage::ObjectSelect(select) => select
                    .object_data
                    .iter()
                    .map(|block| block.object_local_id)
                    .collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    /// A list longer than one message states is split at the block limit, in
    /// order, with nothing lost or repeated, and every part encodes.
    #[test]
    fn a_long_list_is_split_at_the_block_limit() -> Result<(), String> {
        let ids: Vec<u32> = (0..600).collect();
        let messages =
            fill_messages(ids.clone(), MAX_VARIABLE_BLOCKS, select).map_err(|e| e.to_string())?;
        let parts = selected(&messages);
        assert_eq!(
            parts.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![255, 255, 90]
        );
        assert_eq!(parts.concat(), ids);
        for message in &messages {
            let len = encoded_len(message).map_err(|e| e.to_string())?;
            assert!(len <= MTU_BYTES, "{len} bytes");
        }
        Ok(())
    }

    /// A smaller limit — the reference's 254 objects to a selection message —
    /// is honoured, and no blocks make no messages.
    #[test]
    fn a_caller_limit_is_honoured_and_nothing_sends_nothing() -> Result<(), String> {
        let messages = fill_messages((0..300).collect(), 254, select).map_err(|e| e.to_string())?;
        assert_eq!(
            selected(&messages).iter().map(Vec::len).collect::<Vec<_>>(),
            vec![254, 46]
        );
        let none = fill_messages(Vec::new(), 254, select).map_err(|e| e.to_string())?;
        assert_eq!(none, Vec::new());
        let one = fill_messages_or_one_empty(Vec::new(), 254, select).map_err(|e| e.to_string())?;
        assert_eq!(selected(&one), vec![Vec::<u32>::new()]);
        Ok(())
    }

    /// Blocks larger than a few bytes fill the datagram before the count byte:
    /// a list of long names is cut where the next block would pass the MTU, and
    /// a block too big for any datagram still goes, alone.
    #[test]
    fn the_mtu_cuts_a_list_of_large_blocks() -> Result<(), String> {
        let name = |local_id: u32, len: usize| ObjectNameObjectDataBlock {
            local_id,
            name: vec![b'n'; len],
        };
        let build = |object_data| {
            AnyMessage::ObjectName(ObjectName {
                agent_data: ObjectNameAgentDataBlock {
                    agent_id: Uuid::nil(),
                    session_id: Uuid::nil(),
                },
                object_data,
            })
        };
        let blocks: Vec<_> = (0..20).map(|id| name(id, 200)).collect();
        let messages =
            fill_messages(blocks, MAX_VARIABLE_BLOCKS, build).map_err(|e| e.to_string())?;
        let mut total = 0;
        for message in &messages {
            let len = encoded_len(message).map_err(|e| e.to_string())?;
            assert!(len <= MTU_BYTES, "{len} bytes");
            if let AnyMessage::ObjectName(message) = message {
                total += message.object_data.len();
            }
        }
        assert_eq!(total, 20);
        assert!(messages.len() > 1);
        Ok(())
    }

    /// A removal of more folders and items than one message carries is split
    /// with both lists inside their limits, folders first, nothing lost.
    #[test]
    fn a_large_inventory_removal_is_split_across_messages() -> Result<(), String> {
        let folders: Vec<Uuid> = (0..300).map(Uuid::from_u128).collect();
        let items: Vec<Uuid> = (1000..1400).map(Uuid::from_u128).collect();
        let messages =
            remove_inventory_objects(Uuid::nil(), Uuid::nil(), folders.clone(), items.clone())
                .map_err(|e| e.to_string())?;
        let mut seen_folders = Vec::new();
        let mut seen_items = Vec::new();
        for message in &messages {
            let len = encoded_len(message).map_err(|e| e.to_string())?;
            assert!(len <= MTU_BYTES, "{len} bytes");
            let AnyMessage::RemoveInventoryObjects(message) = message else {
                return Err(format!("not a RemoveInventoryObjects: {message:?}"));
            };
            assert!(message.folder_data.len() + message.item_data.len() <= MAX_VARIABLE_BLOCKS);
            seen_folders.extend(message.folder_data.iter().map(|block| block.folder_id));
            seen_items.extend(message.item_data.iter().map(|block| block.item_id));
        }
        assert_eq!(seen_folders, folders);
        assert_eq!(seen_items, items);
        let none = remove_inventory_objects(Uuid::nil(), Uuid::nil(), [], [])
            .map_err(|e| e.to_string())?;
        assert_eq!(none, Vec::new());
        Ok(())
    }
}
