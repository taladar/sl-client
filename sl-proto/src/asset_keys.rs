//! Local persistent-asset key newtypes that have no `sl-types` equivalent.
//!
//! `sl-types` provides keys for the entities it models ([`AgentKey`], [`ObjectKey`],
//! [`TextureKey`], [`MeshKey`], …), but not for the *generic* asset id used by the
//! legacy transfer/upload path, nor for the animation asset id carried by
//! `AgentAnimation`. Those two roles are distinct — a generic asset id can name
//! any [`AssetType`](crate::AssetType), an [`AnimationKey`] only ever names an
//! animation asset — so they live here as their own newtypes rather than as a
//! bare [`Uuid`], mirroring the `sl-types` key wrappers (a `uuid()` accessor and
//! `From<Uuid>`). The wire codec only ever sees the bare [`Uuid`].
//!
//! [`AgentKey`]: sl_types::key::AgentKey
//! [`ObjectKey`]: sl_types::key::ObjectKey
//! [`TextureKey`]: sl_types::key::TextureKey
//! [`MeshKey`]: sl_types::key::MeshKey

use uuid::Uuid;

/// The id of a generic asset, as named by the legacy UDP `TransferRequest` /
/// `AssetUploadRequest` path (which is generic over the asset's
/// [`AssetType`](crate::AssetType)).
///
/// Distinct from the typed [`TextureKey`](sl_types::key::TextureKey) /
/// [`MeshKey`](sl_types::key::MeshKey): those name a specific asset class fetched
/// over its own capability, whereas an `AssetKey` is the class-agnostic id paired
/// with an explicit `AssetType` at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct AssetKey(pub Uuid);

impl AssetKey {
    /// Wraps a raw asset `Uuid`.
    #[must_use]
    pub const fn new(id: Uuid) -> Self {
        Self(id)
    }

    /// The raw asset `Uuid` (the value the wire codec sees).
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for AssetKey {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for AssetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The id of an animation asset, as carried by `AgentAnimation` (the asset the
/// agent starts or stops playing on itself).
///
/// A dedicated newtype keeps an animation id from being transposed with a
/// generic [`AssetKey`] or any other key at a `play`/`stop` call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct AnimationKey(pub Uuid);

impl AnimationKey {
    /// Wraps a raw animation-asset `Uuid`.
    #[must_use]
    pub const fn new(id: Uuid) -> Self {
        Self(id)
    }

    /// The raw animation-asset `Uuid` (the value the wire codec sees).
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for AnimationKey {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for AnimationKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{AnimationKey, AssetKey};
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    #[test]
    fn asset_key_round_trips() {
        let id = Uuid::from_u128(0x42);
        let key = AssetKey::from(id);
        assert_eq!(key.uuid(), id);
        assert_eq!(AssetKey::new(id), key);
        assert_eq!(key.to_string(), id.to_string());
    }

    #[test]
    fn animation_key_round_trips() {
        let id = Uuid::from_u128(0x99);
        let key = AnimationKey::from(id);
        assert_eq!(key.uuid(), id);
        assert_eq!(AnimationKey::new(id), key);
    }
}
