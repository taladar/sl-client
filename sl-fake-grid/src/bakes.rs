//! Who composites an avatar's skin: the grid, or every viewer that looks at
//! it.
//!
//! Second Life central-bakes ("Sunshine"): the simulator composites the worn
//! layers into the baked slots and publishes the result, and a viewer fetches
//! each baked slot **by URL** from the grid's appearance service
//! (`LLVOAvatar::getImageURL`) rather than by asset id. A stock OpenSim region
//! runs no such service: each viewer composites its own agent's layers, uploads
//! the result as an ordinary texture asset (`UploadBakedTexture`), and every
//! other viewer fetches those ids the way it fetches any other texture.
//!
//! # Why this is a policy and not a URL
//!
//! Dropping the appearance service on its own is **worse than leaving it**, and
//! that is the whole reason this is a type. A viewer decides per avatar whether
//! that avatar is server-baked; once it has decided so, `getImageURL` is the
//! only way it will ever ask for a baked slot, and with no service URL to build
//! from that function returns an empty string — no request, no failure, no
//! warning. The avatar stays a cloud forever and nothing in the log says why.
//!
//! So the decision has to flip with the service. What a viewer reads to make it:
//!
//! | input | where it comes from | server-baked | client-baked |
//! | --- | --- | --- | --- |
//! | the appearance service URL | the login response's `agent_appearance_service` | named | absent |
//! | the region's central-bake protocol bit | `RegionHandshake`'s `RegionInfo4.RegionProtocols`, bit 0 ([`REGION_PROTOCOL_SERVER_BAKES`]) | set | clear |
//! | the per-avatar appearance version | the `AvatarAppearance` message's `AppearanceData` block | present, version 1 | **no block at all** |
//! | the bake trigger capability | the seed grant's `UpdateAvatarAppearance` | granted | withheld |
//!
//! The middle two are the ones a "just drop the URL" change would have missed.
//! `RegionProtocols` bit 0 is what `LLViewerRegion` turns into
//! `getCentralBakeVersion()`, and it gates the *agent's own* half: a viewer in a
//! region that claims to central-bake never sends `AgentSetAppearance` and never
//! uploads a bake of its own, so a grid that sets the bit and then serves no
//! appearance service has an agent that can neither be baked nor bake itself.
//! `setIsUsingServerBakes(appearance_version > 0)` in
//! `LLVOAvatar::processAvatarAppearance` is the per-avatar half, and it is the
//! one that decides `getImageURL`.
//!
//! # What the other side is, measured
//!
//! Every client-baked answer here is what OpenSim's `LLClientView` actually
//! writes, not a guess at the opposite of Second Life:
//!
//! - `SendRegionHandshake` writes `RegionProtocols = 1 << 63` — bit 0 clear,
//!   and the top bit set for "more than 6 baked textures" (Bakes on Mesh),
//!   which is a separate question from who does the baking and is why
//!   [`ImitatedGrid::region_protocol_bits`](crate::ImitatedGrid::region_protocol_bits)
//!   contributes it rather than this policy.
//! - `SendAppearance` writes a literal zero block count where the
//!   `AppearanceData` block would go — the comment in the source is `// no
//!   AppearanceData` — so an OpenSim avatar's appearance version is not "0", it
//!   is *absent*, and [`BakePolicy::apply`] reproduces that by clearing all
//!   three of the block's fields rather than zeroing them.
//! - OpenSim has no `UpdateAvatarAppearance` capability at all, which the
//!   `server-appearance-bake` conformance case has been measuring against the
//!   live grid since before this policy existed.
//!
//! `SimulatorFeatures` is **not** on that list, which is worth writing down
//! because it looks as though it should be. The only appearance-adjacent key it
//! carries is `BakesOnMeshEnabled`, and that answers a different question —
//! whether a baked slot may reference an ordinary texture — not who composites
//! the bake. Neither flavour sets it today.
//!
//! Second Life's side of the first row is the one derivation: bit 0's meaning
//! ("server side texture baking") is documented in OpenSim's own source next to
//! the write, and Second Life is the grid that does it. Whether Second Life
//! *also* sets bit 63 is unmeasured and so is not claimed — the reference
//! viewer reads that bit as an OpenSim extension (`// OS sets bit 63 when BOM
//! supported`) and decides Bakes on Mesh support another way on Second Life.

use sl_proto::AvatarAppearance;

/// `RegionProtocols` bit 0: this region's grid composites avatar bakes.
///
/// `LLViewerRegion::unpackRegionHandshake` reads it as
/// `mCentralBakeVersion = region_protocols & 1`, which is what
/// `getCentralBakeVersion()` answers everywhere a viewer decides whether to
/// bake and upload an avatar itself.
pub const REGION_PROTOCOL_SERVER_BAKES: u64 = 1;

/// `RegionProtocols` bit 63: this region supports more than six baked textures
/// (Bakes on Mesh).
///
/// An OpenSim extension — `LLClientView.SendRegionHandshake` sets it
/// unconditionally and the reference viewer reads it as one (`// OS sets bit 63
/// when BOM supported`), deciding the same question from the grid's identity on
/// Second Life. It rides in the same field as
/// [`REGION_PROTOCOL_SERVER_BAKES`] and is otherwise unrelated to it.
pub const REGION_PROTOCOL_BAKES_ON_MESH: u64 = 1 << 63;

/// Who composites the avatars this grid serves.
///
/// The default is [`ServerSide`](Self::ServerSide), which is Second Life and
/// the fake grid's own default flavour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BakePolicy {
    /// Second Life: the grid bakes, names an `agent_appearance_service`, sets
    /// the central-bake protocol bit, stamps every `AvatarAppearance` with an
    /// `AppearanceData` block, and grants `UpdateAvatarAppearance`.
    #[default]
    ServerSide,
    /// A stock OpenSim region: no appearance service, no central-bake bit, no
    /// `AppearanceData` block and no bake trigger capability. Baked textures
    /// are ordinary assets the viewer fetches by id — which the fake grid's
    /// fabricated bakes already are, since it puts their bytes in the grid
    /// asset store under the ids the appearance names.
    ClientSide,
}

impl BakePolicy {
    /// Whether the login response names an `agent_appearance_service`, and the
    /// per-session appearance-texture route answers at all.
    #[must_use]
    pub const fn advertises_appearance_service(self) -> bool {
        matches!(self, Self::ServerSide)
    }

    /// Whether the seed grant carries the `UpdateAvatarAppearance` capability —
    /// the POST a viewer triggers a central bake with.
    #[must_use]
    pub const fn grants_bake_capability(self) -> bool {
        matches!(self, Self::ServerSide)
    }

    /// This policy's contribution to a region's `RegionProtocols` field:
    /// [`REGION_PROTOCOL_SERVER_BAKES`] when the grid bakes, nothing when it
    /// does not.
    #[must_use]
    pub const fn region_protocol_bits(self) -> u64 {
        match self {
            Self::ServerSide => REGION_PROTOCOL_SERVER_BAKES,
            Self::ClientSide => 0,
        }
    }

    /// Applies this policy to an `AvatarAppearance` on its way out.
    ///
    /// A fixture describes how an avatar *looks* — its visual params and its
    /// baked slots — and both grids send exactly that. What differs is the
    /// `AppearanceData` block wrapped around it, so the flavour is applied here,
    /// at the send, rather than being baked into every fixture that builds a
    /// record.
    ///
    /// Client-side means the block is **absent**, not zeroed: OpenSim writes a
    /// zero block count, so the three fields go to `None` and
    /// [`SimSession::send_avatar_appearance`](sl_proto::SimSession::send_avatar_appearance)
    /// then omits the block entirely. A zeroed block would say
    /// "`AppearanceVersion` 0", which reaches the reference viewer's
    /// `setIsUsingServerBakes(appearance_version > 0)` as the same answer but
    /// reaches everything that looks for the block itself as a different one.
    pub const fn apply(self, appearance: &mut AvatarAppearance) {
        match self {
            Self::ServerSide => {}
            Self::ClientSide => {
                appearance.appearance_version = None;
                appearance.cof_version = None;
                appearance.appearance_flags = None;
            }
        }
    }

    /// [`apply`](Self::apply) to a record this policy takes ownership of, for
    /// the send sites that build one per avatar.
    #[must_use]
    pub const fn applied(self, mut appearance: AvatarAppearance) -> AvatarAppearance {
        self.apply(&mut appearance);
        appearance
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A record that carries every `AppearanceData` field, so a policy that
    /// clears only some of them is visible.
    fn stamped() -> AvatarAppearance {
        AvatarAppearance {
            avatar_id: sl_proto::AgentKey::from(uuid::Uuid::from_u128(7)),
            is_trial: false,
            texture_entry: sl_proto::TextureEntry { faces: Vec::new() },
            visual_params: Vec::new(),
            appearance_version: Some(1),
            cof_version: Some(4),
            appearance_flags: Some(0),
            hover_height: None,
            attachments: Vec::new(),
        }
    }

    /// The server-baked side leaves the record exactly as the fixture wrote it.
    #[test]
    fn a_baking_grid_forwards_the_appearance_block() {
        assert_eq!(BakePolicy::ServerSide.applied(stamped()), stamped());
    }

    /// The block is removed rather than zeroed — OpenSim writes no block, and
    /// a zero-versioned block is a different message on the wire.
    #[test]
    fn a_client_baking_grid_sends_no_appearance_block() {
        let record = BakePolicy::ClientSide.applied(stamped());
        assert_eq!(record.appearance_version, None);
        assert_eq!(record.cof_version, None);
        assert_eq!(record.appearance_flags, None);
        // Everything that describes how the avatar *looks* is untouched: both
        // grids publish baked ids, they only disagree about how they are
        // fetched.
        assert_eq!(record.texture_entry, stamped().texture_entry);
        assert_eq!(record.visual_params, stamped().visual_params);
    }

    /// The central-bake bit is the one a viewer turns into
    /// `getCentralBakeVersion()`, and the two policies have to disagree about
    /// it or the region still claims to bake.
    #[test]
    fn only_a_baking_grid_sets_the_central_bake_bit() {
        assert_eq!(
            BakePolicy::ServerSide.region_protocol_bits(),
            REGION_PROTOCOL_SERVER_BAKES
        );
        assert_eq!(BakePolicy::ClientSide.region_protocol_bits(), 0);
    }

    /// The service and the capability travel with the policy: either both are
    /// there or neither is, which is what makes the silent cloud unreachable.
    #[test]
    fn the_service_and_the_trigger_travel_together() {
        for policy in [BakePolicy::ServerSide, BakePolicy::ClientSide] {
            assert_eq!(
                policy.advertises_appearance_service(),
                policy.grants_bake_capability(),
                "{policy:?} advertises a service it does not grant a trigger for, or the reverse"
            );
        }
        assert_eq!(BakePolicy::default(), BakePolicy::ServerSide);
    }
}
