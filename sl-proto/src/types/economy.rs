//! Economy and money: balances, transactions, economy data.

use sl_types::key::AgentKey;
use sl_types::money::LindenAmount;
use uuid::Uuid;

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
    pub square_meters_credit: i32,
    /// Land committed in square metres.
    pub square_meters_committed: i32,
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
    /// The source of the funds (the payer).
    pub source_id: Uuid,
    /// Whether the source is a group.
    pub source_is_group: bool,
    /// The destination of the funds (the payee).
    pub dest_id: Uuid,
    /// Whether the destination is a group.
    pub dest_is_group: bool,
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

/// Grid economy prices and the region's object capacity, parsed from an
/// `EconomyData` reply to
/// [`Session::request_economy_data`](crate::Session::request_economy_data). All
/// prices are in L$ unless noted.
#[derive(Debug, Clone, PartialEq)]
pub struct EconomyData {
    /// The region's total object/prim capacity.
    pub object_capacity: i32,
    /// The region's current object/prim count.
    pub object_count: i32,
    /// Price per energy unit.
    pub price_energy_unit: i32,
    /// Price to claim an object.
    pub price_object_claim: i32,
    /// Price charged for a public object decaying.
    pub price_public_object_decay: i32,
    /// Price charged for deleting a public object.
    pub price_public_object_delete: i32,
    /// Price to claim a parcel.
    pub price_parcel_claim: i32,
    /// Multiplier applied to the parcel-claim price.
    pub price_parcel_claim_factor: f32,
    /// Price to upload an asset (texture, sound, animation, mesh).
    pub price_upload: i32,
    /// Price to rent a light source.
    pub price_rent_light: i32,
    /// Minimum L$ charged for a teleport.
    pub teleport_min_price: i32,
    /// Exponent applied to teleport distance for pricing.
    pub teleport_price_exponent: f32,
    /// Energy-efficiency scalar.
    pub energy_efficiency: f32,
    /// Weekly object-rent price.
    pub price_object_rent: f32,
    /// Scale factor applied to object rent.
    pub price_object_scale_factor: f32,
    /// Weekly parcel-rent price.
    pub price_parcel_rent: i32,
    /// Price to create a group.
    pub price_group_create: i32,
}
