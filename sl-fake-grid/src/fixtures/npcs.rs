//! NPC fixtures: other avatars, as content rather than as sessions.
//!
//! The fake grid rezzes only the arriving agent's own avatar and has no
//! inter-session broadcast, so a second logged-in avatar is invisible to the
//! first. Everything a viewer does with *other* people — the body, the bakes,
//! the name tag, the playing animation, the attachment that follows a wearer —
//! is therefore modelled here as scripted content: an [`NpcFixture`] on the
//! region's [`SceneFixtures`](crate::world::SceneFixtures), pushed at every
//! arriving agent in the same burst as the prims.
//!
//! What reaches the wire per NPC, in the order a simulator sends it:
//!
//! - the **avatar object** ([`avatar_prim`]) — the
//!   `LEGACY_AVATAR` pcode body carrying the `FirstName` / `LastName`
//!   name-values the viewer labels it with;
//! - its **`AvatarAppearance`** — the visual params and the per-avatar
//!   `TextureEntry` whose [`avatar_texture`] baked slots name the composited
//!   textures. The bake bytes live in the region's asset store under exactly
//!   those ids ([`bake_assets`](NpcFixture::bake_assets), registered by
//!   [`RegionFixture::into_scenario`](super::RegionFixture::into_scenario)), so
//!   a viewer fetches them with a plain `GetTexture` — the OpenSim path, where
//!   no server-bake service is advertised;
//! - its **`AvatarAnimation`** — the complete set of animations it is playing;
//! - its **attachments** — ordinary child objects whose parent is the NPC's
//!   region-local id and whose state byte carries the attachment point
//!   ([`PrimFixture::attached_to`]).

use sl_proto::{
    AnimationKey, AssetKey, AvatarAppearance, AvatarAttachment, Object, PlayingAnimation,
    RegionLocalObjectId, TextureEntry, TextureFace, avatar_texture,
};
use sl_types::key::{AgentKey, InventoryKey, TextureKey};
use sl_types::lsl::{Rotation, Vector};

use super::prims::PrimFixture;
use crate::world::{AvatarIdentity, avatar_prim};

/// The **default avatar's** transmitted visual params — the body a grid hands
/// an account that has no stored appearance yet ("Ruth").
///
/// Verbatim from OpenSim's `AvatarAppearance.SetDefaultParams`
/// (`OpenSim/Framework/AvatarAppearance.cs`, BSD-licensed), which is what the
/// local test grid actually sends, so a fixture NPC looks exactly like a
/// default OpenSim avatar. Setting every param to the midpoint of its own range
/// instead — the obvious-looking alternative — produces a badly distorted body,
/// because the ranges are not centred on anything an avatar wants to be.
///
/// The vector is 218 bytes and a receiver reads it positionally against its own
/// [transmitted param list], which in the standard `avatar_lad.xml` is 253
/// params: exactly the 218 classic ones (every id below 10000), then the 33
/// physics params (10000–10032) and two more (11000, 11001). So the classic
/// half lands slot for slot and the rest falls back to each param's default —
/// which is what OpenSim itself relies on.
///
/// [transmitted param list]: https://wiki.secondlife.com/wiki/Appearance
pub const DEFAULT_VISUAL_PARAMS: [u8; 218] = [
    33, 61, 85, 23, 58, 127, 63, 85, 63, 42, 0, 85, 63, 36, 85, 95, 153, 63, 34, 0, 63, 109, 88,
    132, 63, 136, 81, 85, 103, 136, 127, 0, 150, 150, 150, 127, 0, 0, 0, 0, 0, 127, 0, 0, 255, 127,
    114, 127, 99, 63, 127, 140, 127, 127, 0, 0, 0, 191, 0, 104, 0, 0, 0, 0, 0, 0, 0, 0, 0, 145,
    216, 133, 0, 127, 0, 127, 170, 0, 0, 127, 127, 109, 85, 127, 127, 63, 85, 42, 150, 150, 150,
    150, 150, 150, 150, 25, 150, 150, 150, 0, 127, 0, 0, 144, 85, 127, 132, 127, 85, 0, 127, 127,
    127, 127, 127, 127, 59, 127, 85, 127, 127, 106, 47, 79, 127, 127, 204, 2, 141, 66, 0, 0, 127,
    127, 0, 0, 0, 0, 127, 0, 159, 0, 0, 178, 127, 36, 85, 131, 127, 127, 127, 153, 95, 0, 140, 75,
    27, 127, 127, 0, 150, 150, 198, 0, 0, 63, 30, 127, 165, 209, 198, 127, 127, 153, 204, 51, 51,
    255, 255, 255, 204, 0, 255, 150, 150, 150, 150, 150, 150, 150, 150, 150, 150, 0, 150, 150, 150,
    150, 150, 0, 127, 127, 150, 150, 150, 150, 150, 150, 150, 150, 0, 0, 150, 51, 132, 150, 150,
    150,
];

/// The side, in pixels, of a fixture's baked textures: what a real grid's
/// bakes are.
///
/// A fixture's bake is a flat colour, so it costs about 300 bytes encoded
/// whatever its size — and undersizing it does not pay: a small texture
/// stretched over a whole avatar body reads as a stuck low-LOD blur in the
/// viewer, because the LOD driver has nothing finer to fetch.
const BAKE_TEXTURE_SIZE: u32 = 512;

/// The tag written into the top 16 bits of a baked-texture id, so a bake id is
/// never mistaken for an agent id and reads as one at a glance
/// (`ba4e<slot>-…`).
const BAKE_ID_TAG: u128 = 0xBA4E;

/// The mask of the low 96 bits a baked-texture id carries its avatar's id in.
const BAKE_ID_AGENT_MASK: u128 = (1_u128 << 96) - 1;

/// One baked avatar texture: which [`avatar_texture`] slot it fills, the id it
/// is served under, and the flat colour its asset is.
///
/// The colour is part of the fixture because the bake's *bytes* are fixture
/// content too — [`NpcAppearance::bake_assets`] encodes each one so the region
/// serves the very texture the appearance names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcBake {
    /// The [`avatar_texture`] baked-slot index this texture fills.
    pub slot: usize,
    /// The id the texture is named by, and served under.
    pub texture: TextureKey,
    /// The flat RGBA colour the texture is.
    pub color: [u8; 4],
}

/// An NPC's appearance: the visual params that shape it and the bakes that
/// paint it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcAppearance {
    /// The transmitted visual params, one quantized byte each, in the
    /// reference viewer's param order.
    pub visual_params: Vec<u8>,
    /// The composited textures the avatar wears.
    pub bakes: Vec<NpcBake>,
}

impl Default for NpcAppearance {
    /// [`default_avatar`](Self::default_avatar): the stock body wearing no
    /// bakes.
    fn default() -> Self {
        Self::default_avatar()
    }
}

impl NpcAppearance {
    /// The default avatar's shape ([`DEFAULT_VISUAL_PARAMS`]) with no bakes. A
    /// viewer renders it in whatever it shows for an un-baked avatar.
    #[must_use]
    pub fn default_avatar() -> Self {
        Self {
            visual_params: DEFAULT_VISUAL_PARAMS.to_vec(),
            bakes: Vec::new(),
        }
    }

    /// The default avatar's shape painted `color`: one solid bake per body
    /// region (head, upper body, lower body), each served under an id derived
    /// from `agent` so no two NPCs share a texture.
    ///
    /// This is the appearance a render test asserts against — the whole body
    /// classifies as one known colour.
    #[must_use]
    pub fn solid(agent: AgentKey, color: [u8; 4]) -> Self {
        Self {
            bakes: [
                avatar_texture::HEAD_BAKED,
                avatar_texture::UPPER_BAKED,
                avatar_texture::LOWER_BAKED,
            ]
            .into_iter()
            .map(|slot| NpcBake {
                slot,
                texture: bake_texture(agent, slot),
                color,
            })
            .collect(),
            ..Self::default_avatar()
        }
    }

    /// The per-avatar `TextureEntry` this appearance sends: every
    /// [`avatar_texture`] slot at the `IMG_DEFAULT_AVATAR` sentinel, with each
    /// bake's slot naming its texture.
    #[must_use]
    pub fn texture_entry(&self) -> TextureEntry {
        let mut entry = TextureEntry {
            faces: vec![
                TextureFace::new(TextureKey::from(avatar_texture::IMG_DEFAULT_AVATAR));
                avatar_texture::COUNT
            ],
        };
        for bake in &self.bakes {
            if let Some(face) = entry.faces.get_mut(bake.slot) {
                face.texture_id = bake.texture;
            }
        }
        entry
    }

    /// The bake assets the region has to serve for this appearance: one
    /// JPEG2000 solid per bake, keyed by the very id the texture entry names.
    /// A colour that fails to encode is skipped with a warning — the avatar
    /// then shows a missing texture, which is a visible failure rather than a
    /// panic in a fixture.
    #[must_use]
    pub fn bake_assets(&self) -> Vec<(AssetKey, Vec<u8>)> {
        self.bakes
            .iter()
            .filter_map(|bake| {
                match sl_test_assets::RgbaImage::solid(BAKE_TEXTURE_SIZE, bake.color).j2c() {
                    Ok(bytes) => Some((AssetKey::from(bake.texture.uuid()), bytes)),
                    Err(error) => {
                        tracing::warn!("encoding the bake for slot {} failed: {error}", bake.slot);
                        None
                    }
                }
            })
            .collect()
    }
}

/// The id an NPC's bake for `slot` is served under: the [`BAKE_ID_TAG`] and
/// the slot in the top 32 bits, the avatar's own id in the low 96. Two
/// different slots therefore differ in the tag word and two different avatars
/// in the id word, so a bake id is stable, is unique per (avatar, slot), and
/// can never be mistaken for an agent id.
fn bake_texture(agent: AgentKey, slot: usize) -> TextureKey {
    let slot = u128::try_from(slot).unwrap_or(0) & 0xFFFF;
    let tag = ((BAKE_ID_TAG << 16_u32) | slot) << 96_u32;
    TextureKey::from(uuid::Uuid::from_u128(
        tag | (agent.uuid().as_u128() & BAKE_ID_AGENT_MASK),
    ))
}

/// Another avatar, as region content: who it is, where it stands, how it
/// looks, what it is playing and what it is wearing.
///
/// Build one with [`new`](Self::new) and chain
/// [`looking`](Self::looking) / [`animating`](Self::animating) /
/// [`wearing`](Self::wearing); put it on a region's
/// [`SceneFixtures::npcs`](crate::world::SceneFixtures::npcs) and the arrival
/// burst does the rest.
#[derive(Debug, Clone, PartialEq)]
pub struct NpcFixture {
    /// The region-local id the avatar object is rezzed with (what its
    /// attachments name as their parent). It must not collide with the
    /// region's prims or with the arriving agent's own avatar.
    pub local_id: RegionLocalObjectId,
    /// Who the NPC is: the agent id and the name the viewer labels it with.
    pub identity: AvatarIdentity,
    /// Where it stands, in region metres (the avatar object's centre).
    pub position: Vector,
    /// Which way it faces.
    pub rotation: Rotation,
    /// How it looks.
    pub appearance: NpcAppearance,
    /// What it is playing, in the order the animations are listed on the wire.
    pub animations: Vec<AnimationKey>,
    /// What it is wearing: built attachment objects, already parented to
    /// [`local_id`](Self::local_id) with their point in the state byte.
    pub attachments: Vec<Object>,
}

/// The identity rotation.
const NO_ROTATION: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

impl NpcFixture {
    /// An NPC called `identity`, standing at `position` facing east, with the
    /// default avatar's appearance, no animations and nothing worn.
    #[must_use]
    pub fn new(local_id: RegionLocalObjectId, identity: AvatarIdentity, position: Vector) -> Self {
        Self {
            local_id,
            identity,
            position,
            rotation: NO_ROTATION,
            appearance: NpcAppearance::default_avatar(),
            animations: Vec::new(),
            attachments: Vec::new(),
        }
    }

    /// Gives the NPC `appearance`.
    #[must_use]
    pub fn looking(mut self, appearance: NpcAppearance) -> Self {
        self.appearance = appearance;
        self
    }

    /// Turns the NPC to `rotation`.
    #[must_use]
    pub const fn rotated(mut self, rotation: Rotation) -> Self {
        self.rotation = rotation;
        self
    }

    /// Adds an animation to the set the NPC is playing.
    #[must_use]
    pub fn animating(mut self, animation: AnimationKey) -> Self {
        self.animations.push(animation);
        self
    }

    /// Dresses the NPC in `attachment`, worn on `point` (an attachment-point
    /// code) at `offset` metres and `rotation` from that point, and keyed on
    /// the inventory item `item` the way a worn item is.
    ///
    /// The prim is parented to this NPC, so a caller builds it exactly as it
    /// would build any other prim — the wearer is filled in here.
    #[must_use]
    pub fn wearing(
        mut self,
        attachment: PrimFixture,
        point: u8,
        item: InventoryKey,
        offset: Vector,
        rotation: Rotation,
    ) -> Self {
        self.attachments.push(
            attachment
                .attached_to(self.local_id, point, item, offset, rotation)
                .build(),
        );
        self
    }

    /// The NPC's agent id.
    #[must_use]
    pub const fn agent_id(&self) -> AgentKey {
        self.identity.agent_id
    }

    /// The avatar object the NPC is rezzed as.
    #[must_use]
    pub fn avatar_prim(&self) -> Object {
        let mut object = avatar_prim(self.local_id, &self.identity, self.position.clone());
        object.motion.rotation = self.rotation.clone();
        object
    }

    /// The `AvatarAppearance` record the simulator pushes for the NPC: its
    /// visual params, its baked texture entry and the attachments it wears.
    #[must_use]
    pub fn appearance_record(&self) -> AvatarAppearance {
        AvatarAppearance {
            avatar_id: self.agent_id(),
            is_trial: false,
            texture_entry: self.appearance.texture_entry(),
            visual_params: self.appearance.visual_params.clone(),
            appearance_version: Some(1),
            cof_version: Some(1),
            appearance_flags: Some(0),
            hover_height: None,
            attachments: self
                .attachments
                .iter()
                .map(|attachment| AvatarAttachment {
                    id: attachment.full_id,
                    attachment_point: attachment.attachment_point_id().unwrap_or(0),
                })
                .collect(),
        }
    }

    /// The animations the NPC is playing, as the wire record: each numbered
    /// from one in list order, none of them triggered by an object.
    #[must_use]
    pub fn playing_animations(&self) -> Vec<PlayingAnimation> {
        self.animations
            .iter()
            .enumerate()
            .map(|(index, animation)| PlayingAnimation {
                anim_id: animation.uuid(),
                sequence_id: i32::try_from(index.saturating_add(1)).unwrap_or(1),
                source_id: None,
            })
            .collect()
    }

    /// Every object the NPC contributes to the region: its avatar body
    /// followed by its attachments — what an object refetch has to answer.
    #[must_use]
    pub fn objects(&self) -> Vec<Object> {
        let mut objects = vec![self.avatar_prim()];
        objects.extend(self.attachments.iter().cloned());
        objects
    }

    /// The bake assets the region has to serve for this NPC.
    #[must_use]
    pub fn bake_assets(&self) -> Vec<(AssetKey, Vec<u8>)> {
        self.appearance.bake_assets()
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_types::key::ObjectKey;

    use super::*;

    /// The zero vector.
    const ZERO: Vector = Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// A fixed agent for the NPC under test.
    fn agent() -> AgentKey {
        AgentKey::from(uuid::Uuid::from_u128(0x0BC1))
    }

    /// The NPC under test: painted blue, playing one animation, wearing one
    /// box on its skull.
    fn npc() -> NpcFixture {
        NpcFixture::new(
            RegionLocalObjectId(0x200),
            AvatarIdentity::new(agent(), "Fixture", "Npc"),
            Vector {
                x: 128.0,
                y: 128.0,
                z: 25.95,
            },
        )
        .looking(NpcAppearance::solid(agent(), sl_test_assets::markers::BLUE))
        .animating(AnimationKey::from(uuid::Uuid::from_u128(0x57A2)))
        .wearing(
            PrimFixture::boxed(
                RegionLocalObjectId(0x201),
                ObjectKey::from(uuid::Uuid::from_u128(0xA77A)),
                agent(),
                ZERO,
                Vector {
                    x: 0.2,
                    y: 0.2,
                    z: 0.2,
                },
            ),
            2,
            InventoryKey::from(uuid::Uuid::from_u128(0x17E)),
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.3,
            },
            NO_ROTATION,
        )
    }

    /// The default-avatar params are the table OpenSim sends, unedited: 218
    /// bytes, ending on the three that shape the default skirt. An accidental
    /// re-wrap or a dropped value shifts every later param onto the wrong
    /// slider, which shows up as a distorted body rather than as an error, so
    /// the table is pinned here.
    #[test]
    fn the_default_params_are_opensims_table() {
        assert_eq!(DEFAULT_VISUAL_PARAMS.len(), 218);
        assert_eq!(
            DEFAULT_VISUAL_PARAMS.first().copied(),
            Some(33),
            "the table starts on the wrong value"
        );
        assert_eq!(
            DEFAULT_VISUAL_PARAMS.get(210..),
            Some([0, 0, 150, 51, 132, 150, 150, 150].as_slice()),
            "the table ends on the wrong values"
        );
        // Nothing here is the midpoint sweep an earlier version used.
        assert!(
            DEFAULT_VISUAL_PARAMS.iter().any(|&byte| byte != 128),
            "the table is a constant sweep"
        );
        assert_eq!(
            NpcAppearance::default_avatar().visual_params,
            DEFAULT_VISUAL_PARAMS.to_vec()
        );
        // A painted NPC keeps the same shape; only its bakes differ.
        assert_eq!(
            NpcAppearance::solid(agent(), sl_test_assets::markers::BLUE).visual_params,
            DEFAULT_VISUAL_PARAMS.to_vec()
        );
    }

    /// The avatar body carries the pcode and the name-values a viewer labels
    /// the NPC with, and stands where the fixture put it.
    #[expect(
        clippy::float_cmp,
        reason = "the position is the exactly-representable constant the \
                  fixture was built with, so exact equality is the test"
    )]
    #[test]
    fn an_npc_rezzes_as_a_named_avatar() {
        let object = npc().avatar_prim();
        assert_eq!(object.pcode, sl_proto::pcode::AVATAR);
        assert_eq!(object.full_id.uuid(), agent().uuid());
        assert_eq!(object.local_id, RegionLocalObjectId(0x200));
        assert_eq!(object.motion.position.z, 25.95);
        assert!(
            object
                .name_values()
                .iter()
                .any(|pair| pair.name == "FirstName" && pair.value == "Fixture"),
            "no FirstName in {:?}",
            object.name_value
        );
    }

    /// The bakes reach the texture entry in their own slots, every other slot
    /// stays at the default-avatar sentinel, and the assets served are keyed
    /// by exactly the ids the entry names.
    #[test]
    fn the_bakes_name_the_assets_the_region_serves() {
        let npc = npc();
        let entry = npc.appearance.texture_entry();
        assert_eq!(entry.faces.len(), avatar_texture::COUNT);
        let head = entry.texture_id(avatar_texture::HEAD_BAKED);
        assert_eq!(
            head,
            Some(bake_texture(agent(), avatar_texture::HEAD_BAKED))
        );
        assert_ne!(
            head,
            Some(TextureKey::from(avatar_texture::IMG_DEFAULT_AVATAR))
        );
        // A slot with no bake is the sentinel, not a hole.
        assert_eq!(
            entry.texture_id(avatar_texture::SKIRT_BAKED),
            Some(TextureKey::from(avatar_texture::IMG_DEFAULT_AVATAR))
        );
        let served: Vec<AssetKey> = npc
            .bake_assets()
            .into_iter()
            .map(|(key, bytes)| {
                assert!(!bytes.is_empty(), "an empty bake asset for {key}");
                key
            })
            .collect();
        for bake in &npc.appearance.bakes {
            assert!(
                served.contains(&AssetKey::from(bake.texture.uuid())),
                "slot {} names a texture nothing serves",
                bake.slot
            );
        }
    }

    /// Two NPCs never share a bake id, so one avatar's colour cannot leak on
    /// to another.
    #[test]
    fn two_npcs_get_their_own_bakes() {
        let other = AgentKey::from(uuid::Uuid::from_u128(0x0BC2));
        let mine = NpcAppearance::solid(agent(), sl_test_assets::markers::BLUE);
        let theirs = NpcAppearance::solid(other, sl_test_assets::markers::BLUE);
        let ids: Vec<TextureKey> = mine.bakes.iter().map(|bake| bake.texture).collect();
        for bake in &theirs.bakes {
            assert!(
                !ids.contains(&bake.texture),
                "both NPCs serve slot {} under {}",
                bake.slot,
                bake.texture
            );
        }
    }

    /// The attachment hangs off the NPC on the point it was worn at, and the
    /// appearance record lists it — the two halves a viewer correlates.
    #[test]
    fn an_attachment_is_parented_and_listed() {
        let npc = npc();
        let attachment = npc
            .attachments
            .first()
            .cloned()
            .unwrap_or_else(|| npc.avatar_prim());
        assert_eq!(attachment.parent_id, npc.local_id);
        assert_eq!(attachment.attachment_point_id(), Some(2));
        assert_eq!(
            npc.appearance_record().attachments,
            vec![AvatarAttachment {
                id: attachment.full_id,
                attachment_point: 2,
            }]
        );
        // The refetch answer is the body followed by what it wears.
        let objects = npc.objects();
        assert_eq!(objects.len(), 2);
        assert_eq!(
            objects.first().map(|body| body.local_id),
            Some(npc.local_id)
        );
    }

    /// The animations are numbered from one in list order — a viewer tells a
    /// restart from a re-listing by that sequence id.
    #[test]
    fn the_animations_are_numbered_in_order() {
        let second = AnimationKey::from(uuid::Uuid::from_u128(0x57A3));
        let npc = npc().animating(second);
        let played: Vec<(uuid::Uuid, i32)> = npc
            .playing_animations()
            .into_iter()
            .map(|animation| (animation.anim_id, animation.sequence_id))
            .collect();
        assert_eq!(
            played,
            vec![
                (uuid::Uuid::from_u128(0x57A2), 1),
                (uuid::Uuid::from_u128(0x57A3), 2)
            ]
        );
    }
}
