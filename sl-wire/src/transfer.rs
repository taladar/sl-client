//! The `Params` sub-codecs of the legacy UDP asset **Transfer** protocol
//! (`TransferRequest` → `TransferInfo` + `TransferPacket` stream).
//!
//! The Transfer channel is the old generic asset download path. Plain
//! asset-by-id downloads (`LLTST_ASSET`) are superseded on both Second Life
//! and OpenSim by the `ViewerAsset` HTTP capability, but two source types
//! remain UDP-only on **both** grids — neither has any HTTP capability:
//!
//! - `LLTST_SIM_INV_ITEM` (3): a task-inventory item's asset — how a viewer
//!   reads a script or notecard body out of a prim's contents.
//! - `LLTST_SIM_ESTATE` (4): an estate asset — the estate covenant notecard.
//!
//! Each `TransferRequest` names its source type and carries a source-specific
//! opaque `Params` blob; the serving side echoes the same blob back in its
//! `TransferInfo`. The blobs are `LLDataPackerBinaryBuffer` layouts — raw
//! 16-byte UUIDs and little-endian `S32`s — cross-checked against the
//! reference viewer's `LLTransferSourceParamsInvItem::packParams` /
//! `LLTransferSourceParamsEstate::packParams`
//! (`indra/llmessage/lltransfermanager.cpp`) and OpenSim's
//! `LLClientView.MakeAssetRequest` offsets.

use uuid::Uuid;

use crate::error::WireError;
use crate::field::{Reader, Writer};

/// The `TransferRequest`/`TransferInfo` `ChannelType` for asset transfers
/// (`LLTCT_ASSET`). The misc channel (1) is unused by both grids.
pub const TRANSFER_CHANNEL_ASSET: i32 = 2;

/// The `SourceType` of a plain asset-by-id download (`LLTST_ASSET`) —
/// legacy-superseded by the `ViewerAsset` HTTP capability on both grids.
pub const TRANSFER_SOURCE_ASSET: i32 = 2;

/// The `SourceType` of a task-inventory item asset download
/// (`LLTST_SIM_INV_ITEM`), still UDP-only on both grids.
pub const TRANSFER_SOURCE_SIM_INV_ITEM: i32 = 3;

/// The `SourceType` of an estate asset download (`LLTST_SIM_ESTATE`), still
/// UDP-only on both grids.
pub const TRANSFER_SOURCE_SIM_ESTATE: i32 = 4;

/// The `EstateAssetType` of the estate covenant notecard (`ET_Covenant`), the
/// only estate asset type the reference viewer requests.
pub const ESTATE_ASSET_COVENANT: i32 = 0;

/// The `Params` blob of a `LLTST_SIM_INV_ITEM` transfer — a task-inventory
/// item's asset (script/notecard body in a prim's contents). 100 bytes on the
/// wire: six raw UUIDs at offsets 0/16/32/48/64/80 and a little-endian `S32`
/// asset-type code at offset 96.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransferSourceParamsInvItem {
    /// The requesting agent.
    pub agent_id: Uuid,
    /// The requesting agent's session.
    pub session_id: Uuid,
    /// The owner the requester believes the asset has (the reference viewer
    /// passes the item's owner; OpenSim ignores it).
    pub owner_id: Uuid,
    /// The in-world object (prim) whose task inventory holds the item.
    pub task_id: Uuid,
    /// The task-inventory item whose asset is requested.
    pub item_id: Uuid,
    /// The item's asset id, as the requester knows it.
    pub asset_id: Uuid,
    /// The `LLAssetType` code of the asset.
    pub asset_type: i32,
}

impl TransferSourceParamsInvItem {
    /// Encodes the params blob in the exact `packParams` layout.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.put_uuid(self.agent_id);
        writer.put_uuid(self.session_id);
        writer.put_uuid(self.owner_id);
        writer.put_uuid(self.task_id);
        writer.put_uuid(self.item_id);
        writer.put_uuid(self.asset_id);
        writer.put_i32(self.asset_type);
        writer.into_bytes()
    }

    /// Decodes a params blob (the inverse of [`encode`](Self::encode)).
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if the blob is shorter than the
    /// 100-byte layout.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let mut reader = Reader::new(bytes);
        Ok(Self {
            agent_id: reader.uuid()?,
            session_id: reader.uuid()?,
            owner_id: reader.uuid()?,
            task_id: reader.uuid()?,
            item_id: reader.uuid()?,
            asset_id: reader.uuid()?,
            asset_type: reader.i32()?,
        })
    }
}

/// The `Params` blob of a `LLTST_SIM_ESTATE` transfer — an estate asset (the
/// covenant notecard). 36 bytes on the wire: two raw UUIDs at offsets 0/16 and
/// a little-endian `S32` estate-asset-type code at offset 32. Unlike the
/// task-item params there is no asset id — the simulator resolves the estate's
/// current covenant itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransferSourceParamsEstate {
    /// The requesting agent.
    pub agent_id: Uuid,
    /// The requesting agent's session.
    pub session_id: Uuid,
    /// The `EstateAssetType` code ([`ESTATE_ASSET_COVENANT`] is the only one
    /// the reference viewer uses).
    pub estate_asset_type: i32,
}

impl TransferSourceParamsEstate {
    /// Encodes the params blob in the exact `packParams` layout.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.put_uuid(self.agent_id);
        writer.put_uuid(self.session_id);
        writer.put_i32(self.estate_asset_type);
        writer.into_bytes()
    }

    /// Decodes a params blob (the inverse of [`encode`](Self::encode)).
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if the blob is shorter than the
    /// 36-byte layout.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let mut reader = Reader::new(bytes);
        Ok(Self {
            agent_id: reader.uuid()?,
            session_id: reader.uuid()?,
            estate_asset_type: reader.i32()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{TransferSourceParamsEstate, TransferSourceParamsInvItem};

    /// The task-item params encode to the exact 100-byte `packParams` layout —
    /// six raw UUIDs at offsets 0/16/32/48/64/80, then the asset-type `S32`
    /// little-endian at offset 96 — and decode back identically.
    #[test]
    fn inv_item_params_layout_and_round_trip() {
        let params = TransferSourceParamsInvItem {
            agent_id: Uuid::from_u128(0x0101_0101_0101_0101_0101_0101_0101_0101),
            session_id: Uuid::from_u128(0x0202_0202_0202_0202_0202_0202_0202_0202),
            owner_id: Uuid::from_u128(0x0303_0303_0303_0303_0303_0303_0303_0303),
            task_id: Uuid::from_u128(0x0404_0404_0404_0404_0404_0404_0404_0404),
            item_id: Uuid::from_u128(0x0505_0505_0505_0505_0505_0505_0505_0505),
            asset_id: Uuid::from_u128(0x0606_0606_0606_0606_0606_0606_0606_0606),
            // AT_LSL_TEXT; 10 exercises a multi-byte little-endian encoding
            // check below via a distinct value too.
            asset_type: 10,
        };
        let bytes = params.encode();
        assert_eq!(bytes.len(), 100);
        assert_eq!(
            bytes.get(0..16),
            Some(params.agent_id.as_bytes().as_slice())
        );
        assert_eq!(
            bytes.get(16..32),
            Some(params.session_id.as_bytes().as_slice())
        );
        assert_eq!(
            bytes.get(32..48),
            Some(params.owner_id.as_bytes().as_slice())
        );
        assert_eq!(
            bytes.get(48..64),
            Some(params.task_id.as_bytes().as_slice())
        );
        assert_eq!(
            bytes.get(64..80),
            Some(params.item_id.as_bytes().as_slice())
        );
        assert_eq!(
            bytes.get(80..96),
            Some(params.asset_id.as_bytes().as_slice())
        );
        // S32 little-endian: 10 = 0x0000000a.
        assert_eq!(bytes.get(96..100), Some([0x0a, 0, 0, 0].as_slice()));
        assert_eq!(TransferSourceParamsInvItem::decode(&bytes), Ok(params));
        // A truncated blob is a hard error, not a zero-filled struct.
        assert!(matches!(
            TransferSourceParamsInvItem::decode(bytes.get(0..99).unwrap_or(&[])),
            Err(crate::WireError::UnexpectedEof { .. })
        ));
    }

    /// The estate params encode to the exact 36-byte `packParams` layout — two
    /// raw UUIDs then the estate-asset-type `S32` little-endian — and decode
    /// back identically.
    #[test]
    fn estate_params_layout_and_round_trip() {
        let params = TransferSourceParamsEstate {
            agent_id: Uuid::from_u128(0x1111_1111_1111_1111_1111_1111_1111_1111),
            session_id: Uuid::from_u128(0x2222_2222_2222_2222_2222_2222_2222_2222),
            estate_asset_type: super::ESTATE_ASSET_COVENANT,
        };
        let bytes = params.encode();
        assert_eq!(bytes.len(), 36);
        assert_eq!(
            bytes.get(0..16),
            Some(params.agent_id.as_bytes().as_slice())
        );
        assert_eq!(
            bytes.get(16..32),
            Some(params.session_id.as_bytes().as_slice())
        );
        assert_eq!(bytes.get(32..36), Some([0, 0, 0, 0].as_slice()));
        assert_eq!(TransferSourceParamsEstate::decode(&bytes), Ok(params));
        assert!(matches!(
            TransferSourceParamsEstate::decode(bytes.get(0..35).unwrap_or(&[])),
            Err(crate::WireError::UnexpectedEof { .. })
        ));
    }
}
