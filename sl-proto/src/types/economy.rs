//! Economy and money: balances, transactions, economy data.

use sl_types::key::{AgentKey, OwnerKey};
use sl_types::money::LindenAmount;
use uuid::Uuid;

use crate::types::LandArea;

/// The agent's L$ balance and land-tier accounting, parsed from a
/// `MoneyBalanceReply` (a reply to
/// [`Session::request_money_balance`](crate::Session::request_money_balance), or
/// pushed unsolicited by the simulator after a transaction changes the balance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoneyBalance {
    /// The agent the balance belongs to (the client's own id).
    pub agent_id: AgentKey,
    /// The id of the transaction that triggered this reply, correlating it back to
    /// the pay/buy that caused it (e.g. the `TransactionID` echoed by a
    /// [`Session::send_money_transfer`](crate::Session::send_money_transfer)). Nil
    /// for a plain unsolicited balance poll, which has no triggering transaction.
    pub transaction_id: Uuid,
    /// Whether the transaction that triggered this reply succeeded. Always `true`
    /// for a plain balance poll.
    pub success: bool,
    /// The current L$ balance.
    pub balance: LindenAmount,
    /// Land credit in square metres (owned-land tier accounting).
    pub square_meters_credit: LandArea,
    /// Land committed in square metres.
    pub square_meters_committed: LandArea,
    /// A human-readable description of the triggering transaction (empty for a
    /// plain balance poll).
    pub description: String,
    /// Details of the transaction that changed the balance, present only when the
    /// reply carried a non-zero `TransactionInfo` block (servers ≥ 1.40); `None`
    /// for a plain balance poll.
    pub transaction: Option<MoneyTransaction>,
}

/// The transaction details optionally attached to a [`MoneyBalance`], describing
/// the L$ movement that changed the balance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoneyTransaction {
    /// The transaction type code (e.g. `5008` for paying an object); classify
    /// with [`MoneyTransactionType::from_i32`].
    pub transaction_type: i32,
    /// The source of the funds (the payer) — an agent or a group.
    pub source: OwnerKey,
    /// The destination of the funds (the payee) — an agent or a group.
    pub dest: OwnerKey,
    /// The L$ amount moved.
    pub amount: LindenAmount,
    /// A description of the item or reason for the transaction.
    pub item_description: String,
}

/// The kind of an L$ transfer, used as the `TransactionType` of a
/// [`Session::send_money_transfer`](crate::Session::send_money_transfer). A small
/// subset of the Second Life transaction codes (`lltransactiontypes.h`); any
/// other code round-trips through [`MoneyTransactionType::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MoneyTransactionType {
    /// A direct L$ gift to another avatar (`5001`).
    Gift,
    /// Paying a scripted object — a tip jar, vendor, pay button, etc. (`5008`).
    PayObject,
    /// Buying an object that is set for sale (`5000`).
    ObjectSale,
    /// Any other transaction code, preserved verbatim.
    Other(i32),
}

impl MoneyTransactionType {
    /// Classifies a `TransactionType` wire value.
    #[must_use]
    pub const fn from_i32(value: i32) -> Self {
        match value {
            5000 => Self::ObjectSale,
            5001 => Self::Gift,
            5008 => Self::PayObject,
            other => Self::Other(other),
        }
    }

    /// The wire value for this transaction type.
    #[must_use]
    pub const fn to_i32(self) -> i32 {
        match self {
            Self::ObjectSale => 5000,
            Self::Gift => 5001,
            Self::PayObject => 5008,
            Self::Other(code) => code,
        }
    }
}

/// A Land Impact amount: the resource cost a single object contributes to a
/// region or parcel's object budget, and the unit that budget is denominated in.
///
/// Land Impact (LI) generalises the legacy prim count. Since mesh, an object's
/// contribution is the maximum of its streaming (download), physics, and server
/// weights, with each legacy prim counting as 1 LI; a region's total budget and
/// current usage — the [`object_capacity`](EconomyData::object_capacity) and
/// [`object_count`](EconomyData::object_count) of [`EconomyData`] — are
/// expressed in these units. On OpenSim the budget is the plain `MaxPrims` prim
/// count (1 prim = 1 LI); on Second Life a full 256×256 region carries a 20 000
/// LI budget.
///
/// The wire carries this as a signed 32-bit integer, but a conforming simulator
/// only ever sends non-negative values, so it is decoded into a `u32` at the
/// codec boundary (`land_impact_from_wire`); a negative value is rejected rather
/// than coerced.
///
/// TODO: move this to `sl_types` (alongside [`LindenAmount`] and `LandArea`) the
/// next time shared value types are migrated there. Kept local to `sl-proto` for
/// now to avoid cutting an `sl-types` release for a single newtype.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct LandImpact(pub u32);

impl std::fmt::Display for LandImpact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(value) = self;
        write!(f, "{value} LI")
    }
}

impl std::ops::Add for LandImpact {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        let Self(lhs) = self;
        let Self(rhs) = rhs;
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "matches the wrapping/overflow behaviour of the same operation on the underlying integer, which is the least surprising result for the caller"
        )]
        Self(lhs + rhs)
    }
}

impl std::ops::Sub for LandImpact {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        let Self(lhs) = self;
        let Self(rhs) = rhs;
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "matches the wrapping/overflow behaviour of the same operation on the underlying integer, which is the least surprising result for the caller"
        )]
        Self(lhs - rhs)
    }
}

impl From<u32> for LandImpact {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<LandImpact> for u32 {
    fn from(value: LandImpact) -> Self {
        let LandImpact(value) = value;
        value
    }
}

/// Grid economy prices and the region's object capacity, parsed from an
/// `EconomyData` reply to
/// [`Session::request_economy_data`](crate::Session::request_economy_data). All
/// prices are in L$ unless noted.
#[derive(Debug, Clone, PartialEq)]
pub struct EconomyData {
    /// The region's total object capacity (its Land Impact budget).
    pub object_capacity: LandImpact,
    /// The region's current object usage (Land Impact in use). Note that some
    /// simulators stub this to zero in the `EconomyData` reply and report live
    /// usage elsewhere (region stats / per-parcel data) instead.
    pub object_count: LandImpact,
    /// Price per energy unit.
    pub price_energy_unit: LindenAmount,
    /// Price to claim an object.
    pub price_object_claim: LindenAmount,
    /// Price charged for a public object decaying.
    pub price_public_object_decay: LindenAmount,
    /// Price charged for deleting a public object.
    pub price_public_object_delete: LindenAmount,
    /// Price to claim a parcel.
    pub price_parcel_claim: LindenAmount,
    /// Multiplier applied to the parcel-claim price.
    pub price_parcel_claim_factor: f32,
    /// Price to upload an asset (texture, sound, animation, mesh).
    pub price_upload: LindenAmount,
    /// Price to rent a light source.
    pub price_rent_light: LindenAmount,
    /// Minimum L$ charged for a teleport.
    pub teleport_min_price: LindenAmount,
    /// Exponent applied to teleport distance for pricing.
    pub teleport_price_exponent: f32,
    /// Energy-efficiency scalar.
    pub energy_efficiency: f32,
    /// Weekly object-rent price.
    pub price_object_rent: f32,
    /// Scale factor applied to object rent.
    pub price_object_scale_factor: f32,
    /// Weekly parcel-rent price.
    pub price_parcel_rent: LindenAmount,
    /// Price to create a group.
    pub price_group_create: LindenAmount,
}
