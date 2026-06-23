//! Discriminated-union UUID keys for wire fields whose referent is one of
//! several roles, selected by a separate discriminator byte/flag on the wire.
//!
//! These mirror the shape of the `sl-types` `*Key` newtypes (and the
//! `sl_types::key::OwnerKey` union), but are **client-only**: they live in
//! `sl-proto`, not `sl-types`. The agent-or-object and item-or-folder unions are
//! general SL concepts that may move into `sl-types` later (bundled with other
//! `sl-types` changes to avoid release churn); the mesh-or-texture pairing and
//! the `MeshKey` newtype are wire-codec details specific to this client.
//!
//! Each union offers [`uuid`](AgentOrObjectKey::uuid)-style accessors so the
//! codec can extract the raw id regardless of which role is set, keeping the
//! on-wire bytes unchanged.

use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, ObjectKey, TextureKey};
use uuid::Uuid;

/// A Second Life *mesh* asset id — the UUID of a mesh asset, as carried in the
/// sculpt/mesh block of a prim whose shape comes from a mesh rather than a
/// sculpt texture (`LL_SCULPT_TYPE_MESH`).
///
/// This is a client-only newtype (it lives in `sl-proto`, not `sl-types`):
/// `sl-types` has a [`TextureKey`] but no dedicated mesh-asset key, and a mesh
/// asset is emphatically not a texture. Its shape mirrors the `sl-types` `*Key`
/// newtypes — `from(uuid)` to construct at the codec boundary,
/// [`uuid`](Self::uuid) to extract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshKey(pub Uuid);

impl MeshKey {
    /// The wrapped raw UUID.
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for MeshKey {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for MeshKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The asset backing a prim's sculpt/mesh shape: either a sculpt **texture**
/// ([`TextureKey`]) or a **mesh** asset ([`MeshKey`]), selected on the wire by
/// the prim's sculpt-type byte (`LL_SCULPT_TYPE_MESH` in the low bits means a
/// mesh, anything else a sculpt texture).
///
/// Typing the id as this union makes a mesh-vs-texture mix-up a compile error,
/// where the raw `LLSculptParams` block carries only a bare UUID plus the
/// sculpt-type discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SculptOrMeshKey {
    /// The prim is a sculpty: the id is a sculpt texture.
    Sculpt(TextureKey),
    /// The prim is a mesh: the id is a mesh asset.
    Mesh(MeshKey),
}

impl SculptOrMeshKey {
    /// The wrapped raw UUID, regardless of whether it is a sculpt texture or a
    /// mesh asset.
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        match self {
            Self::Sculpt(texture) => texture.uuid(),
            Self::Mesh(mesh) => mesh.uuid(),
        }
    }

    /// Whether this id refers to a mesh asset rather than a sculpt texture.
    #[must_use]
    pub const fn is_mesh(&self) -> bool {
        matches!(self, Self::Mesh(_))
    }
}

impl std::fmt::Display for SculptOrMeshKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sculpt(texture) => write!(f, "{texture}"),
            Self::Mesh(mesh) => write!(f, "{mesh}"),
        }
    }
}

/// A chat/source id that is either an **agent** ([`AgentKey`]) or an in-world
/// **object** ([`ObjectKey`]), selected on the wire by a separate source-type
/// discriminator.
///
/// This is a general SL concept (an avatar or a prim can be the source of many
/// things — chat, sounds, effects), so it is a candidate for promotion to
/// `sl-types` alongside [`sl_types::key::OwnerKey`]; for now it stays
/// client-only to avoid churning a `sl-types` release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentOrObjectKey {
    /// The source is an avatar.
    Agent(AgentKey),
    /// The source is an in-world object.
    Object(ObjectKey),
}

impl AgentOrObjectKey {
    /// The wrapped raw UUID, regardless of whether the source is an agent or an
    /// object.
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        match self {
            Self::Agent(agent) => agent.uuid(),
            Self::Object(object) => object.uuid(),
        }
    }

    /// Whether the source is an in-world object rather than an agent.
    #[must_use]
    pub const fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }
}

impl std::fmt::Display for AgentOrObjectKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Agent(agent) => write!(f, "{agent}"),
            Self::Object(object) => write!(f, "{object}"),
        }
    }
}

/// An inventory id that is either an **item** ([`InventoryKey`]) or a whole
/// **folder/category** ([`InventoryFolderKey`]), selected on the wire by a
/// separate asset-type discriminator (`AssetType::Folder` means a folder).
///
/// This is a general SL concept (inventory offers and several other messages
/// carry an id that may be either), so it is a candidate for promotion to
/// `sl-types`; for now it stays client-only to avoid churning a `sl-types`
/// release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InventoryItemOrFolderKey {
    /// The id refers to a single inventory item.
    Item(InventoryKey),
    /// The id refers to a whole inventory folder/category.
    Folder(InventoryFolderKey),
}

impl InventoryItemOrFolderKey {
    /// The wrapped raw UUID, regardless of whether it refers to an item or a
    /// folder.
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        match self {
            Self::Item(item) => item.uuid(),
            Self::Folder(folder) => folder.uuid(),
        }
    }

    /// Whether the id refers to a whole folder rather than a single item.
    #[must_use]
    pub const fn is_folder(&self) -> bool {
        matches!(self, Self::Folder(_))
    }
}

impl std::fmt::Display for InventoryItemOrFolderKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Item(item) => write!(f, "{item}"),
            Self::Folder(folder) => write!(f, "{folder}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, ObjectKey, TextureKey};
    use uuid::Uuid;

    use super::{AgentOrObjectKey, InventoryItemOrFolderKey, MeshKey, SculptOrMeshKey};

    /// A [`MeshKey`] is a transparent wrapper over its [`Uuid`]: wrapping a raw
    /// id and unwrapping it again yields the identical bytes.
    #[test]
    fn mesh_key_round_trips_raw_uuid() {
        for raw in [
            Uuid::nil(),
            Uuid::from_u128(1),
            Uuid::from_u128(0xdead_beef_dead_beef_dead_beef_dead_beef),
        ] {
            assert_eq!(MeshKey::from(raw).uuid(), raw);
        }
    }

    /// A [`SculptOrMeshKey`] exposes the wrapped id regardless of which arm is
    /// set, and reports the mesh arm via [`SculptOrMeshKey::is_mesh`].
    #[test]
    fn sculpt_or_mesh_key_extracts_uuid_and_kind() {
        let raw = Uuid::from_u128(0x5C01);
        let sculpt = SculptOrMeshKey::Sculpt(TextureKey::from(raw));
        let mesh = SculptOrMeshKey::Mesh(MeshKey::from(raw));
        assert_eq!(sculpt.uuid(), raw);
        assert_eq!(mesh.uuid(), raw);
        assert!(!sculpt.is_mesh());
        assert!(mesh.is_mesh());
        // The same id under two arms is a distinct value (the kind is part of it).
        assert_ne!(sculpt, mesh);
    }

    /// An [`AgentOrObjectKey`] exposes the wrapped id regardless of which arm is
    /// set, and reports the object arm via [`AgentOrObjectKey::is_object`].
    #[test]
    fn agent_or_object_key_extracts_uuid_and_kind() {
        let raw = Uuid::from_u128(0xA0B1);
        let agent = AgentOrObjectKey::Agent(AgentKey::from(raw));
        let object = AgentOrObjectKey::Object(ObjectKey::from(raw));
        assert_eq!(agent.uuid(), raw);
        assert_eq!(object.uuid(), raw);
        assert!(!agent.is_object());
        assert!(object.is_object());
        assert_ne!(agent, object);
    }

    /// An [`InventoryItemOrFolderKey`] exposes the wrapped id regardless of which
    /// arm is set, and reports the folder arm via
    /// [`InventoryItemOrFolderKey::is_folder`].
    #[test]
    fn inventory_item_or_folder_key_extracts_uuid_and_kind() {
        let raw = Uuid::from_u128(0x1234_5678);
        let item = InventoryItemOrFolderKey::Item(InventoryKey::from(raw));
        let folder = InventoryItemOrFolderKey::Folder(InventoryFolderKey::from(raw));
        assert_eq!(item.uuid(), raw);
        assert_eq!(folder.uuid(), raw);
        assert!(!item.is_folder());
        assert!(folder.is_folder());
        assert_ne!(item, folder);
    }
}
