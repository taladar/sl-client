//! What a central-baking grid says about an agent's bake in its
//! `UpdateAvatarAppearance` answers (`book/src/gridspec/appearance.md`
//! § Baking, measured by `appearance-baking`).
//!
//! The bake itself — compositing the worn layers, minting the baked textures —
//! is the server's own business; this is the protocol-level description of its
//! result, which [`SimSession::set_baked_outfit`](crate::SimSession::set_baked_outfit)
//! hands the capability so its answers carry what Second Life's carry:
//!
//! - an **accepted** bake states the appearance the grid then sends: its 45
//!   texture-entry ids (`textures`), its visual-parameter bytes
//!   (`visual_params`) and the avatar's bounding box (`avatar_scale`);
//! - a **refused** one dumps the outfit it would have baked: the Current
//!   Outfit Folder's links and the items they resolve to (`cof_raw`), and a
//!   `debug_cof` with the same scale and parameters, each baked slot's id and
//!   layer textures (`slot_textures`), the worn items by wearable type
//!   (`wearables`), the rigged-mesh joint offsets and the attachments' render
//!   cost (`lod`).

use uuid::Uuid;

use crate::session::llsd_map;
use crate::{InventoryItem, Llsd, OwnerKey, WearableType};

/// The number of wearable types a refusal's `wearables` lists (`WT_COUNT`:
/// shape to universal).
pub const WEARABLE_TYPE_COUNT: u8 = 17;

/// One baked slot of a [`BakedOutfit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakedSlot {
    /// The slot's name in the bake service's answers (`head`, `upper`,
    /// `leftarm`, …).
    pub name: &'static str,
    /// The baked texture's id — the grid names a bake by it (`hash_id`).
    pub hash_id: Uuid,
    /// The layer textures the bake was composited from.
    pub texture_ids: Vec<Uuid>,
}

/// An item the outfit dump names: the inventory item, and for a wearable its
/// type and its asset's text (`contents`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakedItem {
    /// The inventory item.
    pub item: InventoryItem,
    /// The agent whose inventory holds it.
    pub agent: Uuid,
    /// For a wearable: its type and the text of its asset.
    pub wearable: Option<(WearableType, String)>,
}

/// The result of a central bake of an agent's outfit, as its
/// `UpdateAvatarAppearance` answers state it.
#[derive(Debug, Clone, PartialEq)]
pub struct BakedOutfit {
    /// The 45 avatar texture-entry ids the appearance carries, in face order.
    pub textures: Vec<Uuid>,
    /// The visual-parameter bytes the appearance carries.
    pub visual_params: Vec<u8>,
    /// The avatar's bounding box, in metres (depth, width, height).
    pub scale: [f32; 3],
    /// Each baked slot.
    pub slots: Vec<BakedSlot>,
    /// The Current Outfit Folder's links and the items they resolve to.
    pub cof_raw: Vec<BakedItem>,
    /// The worn items, by wearable type.
    pub wearables: Vec<BakedItem>,
}

impl BakedOutfit {
    /// The payload of an accepted bake's answer: `avatar_scale`, `textures`,
    /// `visual_params`.
    #[must_use]
    pub fn accepted_payload(&self) -> Vec<(&'static str, Llsd)> {
        vec![
            ("avatar_scale", scale_llsd(self.scale)),
            (
                "textures",
                Llsd::Array(self.textures.iter().map(|id| Llsd::Uuid(*id)).collect()),
            ),
            ("visual_params", params_llsd(&self.visual_params)),
        ]
    }

    /// The payload of a refused bake's answer: `cof_raw` and `debug_cof`.
    #[must_use]
    pub fn refused_payload(&self) -> Vec<(&'static str, Llsd)> {
        let slots = Llsd::Map(
            self.slots
                .iter()
                .map(|slot| {
                    (
                        slot.name.to_owned(),
                        llsd_map(vec![
                            ("hash_id", Llsd::Uuid(slot.hash_id)),
                            (
                                "texture_ids",
                                Llsd::Array(
                                    slot.texture_ids.iter().map(|id| Llsd::Uuid(*id)).collect(),
                                ),
                            ),
                        ]),
                    )
                })
                .collect(),
        );
        let wearables = (0..WEARABLE_TYPE_COUNT)
            .map(|code| {
                Llsd::Array(
                    self.wearables
                        .iter()
                        .filter(|item| {
                            item.wearable
                                .as_ref()
                                .is_some_and(|(kind, _text)| kind.to_code() == code)
                        })
                        .map(item_llsd)
                        .collect(),
                )
            })
            .collect();
        let lod = llsd_map(vec![
            ("animesh_object_count", Llsd::Integer(0)),
            ("attachment_count", Llsd::Integer(0)),
            ("high_lod", Llsd::Real(0.0)),
            ("low_lod", Llsd::Real(0.0)),
            ("lowest_lod", Llsd::Real(0.0)),
            ("medium_lod", Llsd::Real(0.0)),
            ("mesh_count", Llsd::Integer(0)),
            ("mesh_rigged_count", Llsd::Integer(0)),
        ]);
        let debug = llsd_map(vec![
            ("avatar_scale", scale_llsd(self.scale)),
            ("dump_joint_offsets", Llsd::Array(Vec::new())),
            ("lod", lod),
            ("params", params_llsd(&self.visual_params)),
            ("slot_textures", slots),
            ("success", Llsd::Boolean(true)),
            ("warnings", Llsd::Array(Vec::new())),
            ("wearables", Llsd::Array(wearables)),
        ]);
        vec![
            (
                "cof_raw",
                Llsd::Array(self.cof_raw.iter().map(item_llsd).collect()),
            ),
            ("debug_cof", debug),
        ]
    }
}

/// A bounding box as the answers state it: three reals.
fn scale_llsd(scale: [f32; 3]) -> Llsd {
    Llsd::Array(
        scale
            .iter()
            .map(|value| Llsd::Real(f64::from(*value)))
            .collect(),
    )
}

/// Visual-parameter bytes as the answers state them: one integer each.
fn params_llsd(params: &[u8]) -> Llsd {
    Llsd::Array(
        params
            .iter()
            .map(|value| Llsd::Integer(i32::from(*value)))
            .collect(),
    )
}

/// An item as the outfit dump writes it: its ids as strings, its permissions
/// and sale info as maps, and for a wearable its type and asset text.
fn item_llsd(baked: &BakedItem) -> Llsd {
    let item = &baked.item;
    let text = |id: Uuid| Llsd::String(id.to_string());
    let owner = match item.owner {
        OwnerKey::Agent(agent) => agent.uuid(),
        OwnerKey::Group(group) => group.uuid(),
    };
    let permissions = llsd_map(vec![
        (
            "base_mask",
            Llsd::Integer(mask(item.permissions.base.bits())),
        ),
        ("creator_id", text(item.creator_id.uuid())),
        (
            "everyone_mask",
            Llsd::Integer(mask(item.permissions.everyone.bits())),
        ),
        (
            "group_id",
            text(item.group.map_or_else(Uuid::nil, |group| group.uuid())),
        ),
        (
            "group_mask",
            Llsd::Integer(mask(item.permissions.group.bits())),
        ),
        ("last_owner_id", text(item.last_owner_id)),
        (
            "next_owner_mask",
            Llsd::Integer(mask(item.permissions.next_owner.bits())),
        ),
        ("owner_id", text(owner)),
        (
            "owner_mask",
            Llsd::Integer(mask(item.permissions.owner.bits())),
        ),
    ]);
    let sale_info = llsd_map(vec![
        (
            "sale_price",
            Llsd::Integer(
                item.sale_price
                    .as_ref()
                    .map_or(0, |price| i32::try_from(price.0).unwrap_or(i32::MAX)),
            ),
        ),
        ("sale_type", Llsd::Integer(i32::from(item.sale_type))),
    ]);
    let mut entries = vec![
        ("agent_id", text(baked.agent)),
        ("asset_id", text(item.asset_id)),
        ("created_at", Llsd::Integer(item.creation_date)),
        ("desc", Llsd::String(item.description.clone())),
        ("flags", Llsd::Integer(mask(item.flags))),
        ("inv_type", Llsd::Integer(i32::from(item.inv_type))),
        ("item_id", text(item.item_id.uuid())),
        ("metadata", Llsd::Map(std::iter::empty().collect())),
        ("name", Llsd::String(item.name.clone())),
        ("parent_id", text(item.folder_id.uuid())),
        ("permissions", permissions),
        ("sale_info", sale_info),
        ("type", Llsd::Integer(i32::from(item.item_type))),
    ];
    if let Some((kind, contents)) = &baked.wearable {
        entries.push(("contents", Llsd::String(contents.clone())));
        entries.push(("wearable_type", Llsd::Integer(i32::from(kind.to_code()))));
    }
    llsd_map(entries)
}

/// A 32-bit mask as the LLSD integer that carries its bits.
const fn mask(bits: u32) -> i32 {
    i32::from_ne_bytes(bits.to_ne_bytes())
}
