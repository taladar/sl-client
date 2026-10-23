//! What Second Life makes of a notecard body a viewer saves into its own
//! inventory (`UpdateNotecardAgentInventory`).
//!
//! Measured by the `notecards-gestures` census on aditi (2026-10-23,
//! `book/src/gridspec/inventory.md` § Notecards and gestures): a body whose
//! embedded item names an asset the saver holds no item of is refused, in the
//! words of its verifier, and every embedded item of a body it keeps is
//! rewritten — a new item id, and the group, everyone and next-owner masks
//! set to what follows. OpenSim stores the bytes it was sent.
//!
//! The refusal's words are aditi's; the map around them is the shape
//! `NewFileAgentInventory`'s verifier answers in (`sim_new_file`), since the
//! save's own was not recorded.

use crate::sim_caps::CapsResponse;
use crate::sim_session::SimSession;

/// The group mask Second Life writes into an embedded item it keeps.
pub(crate) const SECOND_LIFE_EMBEDDED_GROUP_MASK: u32 = 0x7fff_ffff;

/// The everyone mask Second Life writes into an embedded item it keeps: every
/// bit but modify's.
pub(crate) const SECOND_LIFE_EMBEDDED_EVERYONE_MASK: u32 = 0x7fff_bfff;

/// The next-owner mask Second Life writes into an embedded item it keeps.
pub(crate) const SECOND_LIFE_EMBEDDED_NEXT_OWNER_MASK: u32 = 0x7fff_ffff;

/// What Second Life's verifier says of a body it refuses.
pub(crate) const SECOND_LIFE_NOTECARD_REFUSAL: &str = "Internal error from verifier.";

/// The body Second Life stores for `data` saved by `sim`'s agent, or the
/// answer it refuses it with. A body that is no notecard is stored as sent:
/// nothing measured says otherwise.
pub(crate) fn second_life_body(
    sim: &mut SimSession,
    data: Vec<u8>,
) -> Result<Vec<u8>, CapsResponse> {
    let Ok(mut notecard) = sl_notecard::Notecard::decode(&data) else {
        return Ok(data);
    };
    if notecard.items.is_empty() {
        return Ok(data);
    }
    let held = |asset: uuid::Uuid| {
        sim.agent_inventory()
            .items()
            .any(|item| item.asset_id == asset)
    };
    if notecard.items.iter().any(|item| !held(item.asset_id.0)) {
        return Err(crate::sim_new_file::second_life_refusal(
            200,
            "Upload_InvalidAsset",
            SECOND_LIFE_NOTECARD_REFUSAL,
            Vec::new(),
        ));
    }
    for item in &mut notecard.items {
        item.item_id = sl_types::key::Key(sim.mint_id());
        item.permissions.group_mask = sl_notecard::PermissionMask(SECOND_LIFE_EMBEDDED_GROUP_MASK);
        item.permissions.everyone_mask =
            sl_notecard::PermissionMask(SECOND_LIFE_EMBEDDED_EVERYONE_MASK);
        item.permissions.next_owner_mask =
            sl_notecard::PermissionMask(SECOND_LIFE_EMBEDDED_NEXT_OWNER_MASK);
    }
    Ok(notecard.encode())
}
