//! The account's benefits package: what its subscription level entitles it to.
//!
//! Second Life's login response carries three related fields, and together they
//! are where a modern viewer reads every upload price and most per-account
//! limits — **not** from the `EconomyData` reply, which predates them:
//!
//! - `account_type` names the package the account is on (`"Base"`,
//!   `"Premium"`, `"Premium_Plus"`);
//! - `account_level_benefits` holds *that* package's numbers;
//! - `premium_packages` maps every package name to its numbers, so a viewer can
//!   render "you have N, Premium gives you M" without guessing.
//!
//! # Why this is not `EconomyData`
//!
//! `EconomyData` has one flat `price_upload` for every asset class, and the
//! reference viewer stopped spending it on Second Life: `LLAgentBenefits`
//! answers `getTextureUploadCost` from this package instead, and Firestorm's
//! `OpenSim legacy economy` patches fall back to `EconomyData` **only when the
//! grid is not Second Life**. So the legacy field is the *OpenSim* path, and
//! this is the Second Life one — the opposite way round from how it reads.
//!
//! Two things live here that `EconomyData` cannot express at all. Texture
//! uploads are **tiered**: `large_texture_upload_cost` applies above
//! [`MIN_2K_TEXTURE_AREA`], so a 2048×2048 texture costs more than a 512×512
//! one and a single price cannot say so. And every figure is **per package**,
//! which is the whole point of a subscription.
//!
//! # A stock OpenSim grid sends none of this
//!
//! Not an empty package — no fields at all. Firestorm does not merely tolerate
//! that, it gates the entire benefits init behind `isInSecondLife()`, because
//! the reference parse *fails* when a required field is missing and it requires
//! the `Base` and `Premium` packages to be present. So [`AccountBenefits`] is
//! `None` against OpenSim, and a consumer must have an answer for that rather
//! than treating it as a malformed login.
//!
//! # Strictness
//!
//! The eight integer fields are **required**: the reference parse rejects a
//! whole package when any one is missing, rather than defaulting it, and
//! [`AccountBenefits::from_llsd`] does the same. A package that silently
//! defaulted a missing `texture_upload_cost` to zero would tell a viewer that
//! uploads are free.

use std::collections::{BTreeMap, HashMap};

use sl_types::money::LindenAmount;
use sl_wire::Llsd;

/// The shape an LLSD map arrives in, spelled once so the helpers below read as
/// what they are rather than as their container.
type LlsdMap = HashMap<String, Llsd>;

/// The smallest texture area charged at the large-texture rate: `1024 * 1024 +
/// 1`, i.e. anything **larger** than 1024×1024.
///
/// The reference viewer's `LLAgentBenefits::MIN_2K_TEXTURE_AREA`. Note the
/// `+ 1`: a texture of exactly 1024×1024 pays the ordinary rate, and only
/// something bigger crosses the tier.
pub const MIN_2K_TEXTURE_AREA: u64 = 1024 * 1024 + 1;

/// One subscription package's entitlements, decoded from an
/// `account_level_benefits` (or a `premium_packages[name].benefits`) map.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AccountBenefits {
    /// How many animated objects (animesh) the account may have.
    pub animated_object_limit: i32,
    /// The L$ cost of uploading an animation.
    pub animation_upload_cost: LindenAmount,
    /// How many attachments the account may wear.
    pub attachment_limit: i32,
    /// The L$ cost of creating a group.
    ///
    /// This is the figure the reference viewer actually charges, in place of
    /// [`EconomyData::price_group_create`](crate::EconomyData::price_group_create);
    /// off Second Life it answers `0` regardless.
    pub create_group_cost: LindenAmount,
    /// How many groups the account may belong to.
    pub group_membership_limit: i32,
    /// How many picks the account may publish on its profile.
    pub picks_limit: i32,
    /// The L$ cost of uploading a sound.
    pub sound_upload_cost: LindenAmount,
    /// The L$ cost of uploading a texture at or below
    /// [`MIN_2K_TEXTURE_AREA`].
    pub texture_upload_cost: LindenAmount,
    /// The L$ costs of uploading a texture **above** [`MIN_2K_TEXTURE_AREA`],
    /// ascending.
    ///
    /// Never empty: when the grid sends no `large_texture_upload_cost`, this
    /// holds [`texture_upload_cost`](Self::texture_upload_cost) alone, which is
    /// the reference viewer's own fallback and keeps
    /// [`texture_upload_cost_for`](Self::texture_upload_cost_for) total.
    ///
    /// The reference viewer sorts the array and then only ever reads its first
    /// element, so the extra entries are a tier ladder nothing climbs yet. They
    /// are kept rather than discarded because discarding them would make a
    /// future tier silently unavailable — but see
    /// [`large_texture_upload_cost`](Self::large_texture_upload_cost) for the
    /// one this workspace charges.
    pub large_texture_upload_costs: Vec<LindenAmount>,
}

impl AccountBenefits {
    /// The L$ charged for a texture above [`MIN_2K_TEXTURE_AREA`]: the cheapest
    /// large-texture tier.
    ///
    /// The reference viewer's `get2KTextureUploadCost` sorts the array and
    /// returns `[0]` whatever the area, so the ladder has exactly one rung in
    /// practice. Kept as its own accessor so the day a second rung is used, the
    /// change is here and not spread across callers.
    #[must_use]
    pub fn large_texture_upload_cost(&self) -> LindenAmount {
        self.large_texture_upload_costs
            .first()
            .cloned()
            .unwrap_or_else(|| self.texture_upload_cost.clone())
    }

    /// The L$ charged for uploading a texture of `width` × `height`.
    ///
    /// Above [`MIN_2K_TEXTURE_AREA`] this is
    /// [`large_texture_upload_cost`](Self::large_texture_upload_cost); at or
    /// below it, [`texture_upload_cost`](Self::texture_upload_cost). This is the
    /// number to send as a `NewFileAgentInventory` `expected_upload_cost` on
    /// Second Life — the grid checks it, and the flat
    /// [`EconomyData::price_upload`](crate::EconomyData::price_upload) is not
    /// what it checks against.
    #[must_use]
    pub fn texture_upload_cost_for(&self, width: u32, height: u32) -> LindenAmount {
        // Saturating rather than plain: the product of two `u32`s does fit a
        // `u64`, but saying so with an operator that cannot wrap costs nothing
        // and keeps the claim local. A saturated value is above the tier
        // anyway, so the branch is right either way.
        if u64::from(width).saturating_mul(u64::from(height)) >= MIN_2K_TEXTURE_AREA {
            self.large_texture_upload_cost()
        } else {
            self.texture_upload_cost.clone()
        }
    }

    /// Decodes one benefits map, `None` when a required field is missing.
    ///
    /// All eight integer fields are required, matching the reference parse: it
    /// abandons the whole package on the first missing one rather than
    /// defaulting it, because a package with a defaulted price is a package
    /// that lies about what things cost.
    #[must_use]
    pub fn from_llsd(map: &LlsdMap) -> Option<Self> {
        let texture_upload_cost = cost(map, "texture_upload_cost")?;
        Some(Self {
            animated_object_limit: required_i32(map, "animated_object_limit")?,
            animation_upload_cost: cost(map, "animation_upload_cost")?,
            attachment_limit: required_i32(map, "attachment_limit")?,
            create_group_cost: cost(map, "create_group_cost")?,
            group_membership_limit: required_i32(map, "group_membership_limit")?,
            picks_limit: required_i32(map, "picks_limit")?,
            sound_upload_cost: cost(map, "sound_upload_cost")?,
            large_texture_upload_costs: large_costs(map, texture_upload_cost.clone()),
            texture_upload_cost,
        })
    }
}

/// Reads a required integer benefit field.
///
/// Accepts an LLSD `Integer`, or a `String` that parses as one — the reference
/// viewer reads these through `asInteger()`, which coerces a numeric string, and
/// a grid quoting `"10"` for a price is quoting ten.
///
/// A `Real` is deliberately **not** accepted, which is one coercion narrower
/// than `asInteger()`. Every field here is a count or an L$ amount, no grid has
/// been seen sending a fractional one, and honouring it would mean narrowing a
/// `f64` to an `i32` — which this workspace's lints forbid unchecked and which
/// would have to invent a rounding rule the reference does not document. A
/// grid that starts sending reals should fail loudly here rather than have a
/// rounding guess baked in.
fn required_i32(map: &LlsdMap, key: &str) -> Option<i32> {
    match map.get(key)? {
        Llsd::Integer(value) => Some(*value),
        Llsd::String(value) => value.parse().ok(),
        _other => None,
    }
}

/// An L$ amount from a benefit field, clamping the reference's `-1`
/// "unset" sentinel to zero.
///
/// `LLAgentBenefits` initialises every one of these to `-1` before parsing, and
/// a grid could echo that. [`LindenAmount`] is unsigned, and the honest reading
/// of a negative *price* here is "nothing to charge" rather than a debt — the
/// opposite of
/// [`EconomyData::price_group_create`](crate::EconomyData::price_group_create),
/// where the same `-1` had to stay distinguishable because it is what a whole
/// live grid sends for a field nobody spends. Here a negative would be a
/// malformed package, and zero is the safe reading of one.
fn cost(map: &LlsdMap, key: &str) -> Option<LindenAmount> {
    let value = required_i32(map, key)?;
    Some(LindenAmount(u64::try_from(value).unwrap_or(0)))
}

/// Reads `large_texture_upload_cost`, ascending, falling back to `flat`.
///
/// The reference viewer sorts the array and, when it is absent or empty, pushes
/// the flat texture cost so the list is never empty.
fn large_costs(map: &LlsdMap, flat: LindenAmount) -> Vec<LindenAmount> {
    let mut costs: Vec<LindenAmount> = match map.get("large_texture_upload_cost") {
        Some(Llsd::Array(values)) => values
            .iter()
            .filter_map(|value| match value {
                Llsd::Integer(amount) => u64::try_from(*amount).ok(),
                Llsd::String(amount) => amount.parse().ok(),
                _other => None,
            })
            .map(LindenAmount)
            .collect(),
        _absent_or_not_an_array => Vec::new(),
    };
    costs.sort();
    if costs.is_empty() {
        costs.push(flat);
    }
    costs
}

/// Decodes a `premium_packages` map: package name to that package's benefits.
///
/// Each entry nests its numbers under a `benefits` key. A package that fails to
/// decode is **skipped** rather than failing the whole map — unlike a missing
/// field within one package — because a grid that adds a package this build
/// does not understand should cost the viewer the packages it *does*
/// understand.
#[must_use]
pub fn packages_from_llsd(map: &LlsdMap) -> BTreeMap<String, AccountBenefits> {
    map.iter()
        .filter_map(|(name, value)| {
            let Llsd::Map(entry) = value else {
                return None;
            };
            let Llsd::Map(benefits) = entry.get("benefits")? else {
                return None;
            };
            Some((name.clone(), AccountBenefits::from_llsd(benefits)?))
        })
        .collect()
}
