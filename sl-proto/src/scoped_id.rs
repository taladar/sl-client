//! Client-side circuit-instance identity ([`CircuitId`]) and the scoped
//! region-local ids ([`ScopedObjectId`] / [`ScopedParcelId`]) that pair it with
//! an object or parcel id.
//!
//! A region-local id (`RegionLocalObjectId` / `RegionLocalParcelId`) is only
//! meaningful *within one simulator/circuit*: the object cache is partitioned
//! per circuit, region-local ids are recycled and reassigned on a region
//! restart, and a reconnect (even to the same address serving the same region)
//! invalidates whatever bindings the client had learned. Pairing each id with
//! the [`CircuitId`] of the circuit instance it was learned on turns "used an id
//! captured in region A against region B" — and "used an id from before a
//! reconnect" — into a mismatch the [`Session`](crate::Session) can detect,
//! instead of a silent one the type system could not catch.

use sl_wire::{RegionLocalObjectId, RegionLocalParcelId};

/// An opaque, client-assigned identity for one **circuit instance** — a single
/// established connection to a simulator.
///
/// A fresh `CircuitId` is minted every time the [`Session`](crate::Session)
/// establishes a circuit: the root circuit at login, a child-agent circuit to a
/// neighbour, or a brand-new root after a teleport that had no pre-opened child.
/// A child promoted to root across a region border keeps its `CircuitId` (it is
/// the *same* connection instance).
///
/// It is deliberately **not** derived from the simulator address or the region
/// handle: a reconnect to the *same* address serving the *same* region gets a
/// *different* `CircuitId`. That is what makes a stale [`ScopedObjectId`] /
/// [`ScopedParcelId`] captured before the reconnect fail to resolve, rather than
/// silently target whatever now lives at that address.
///
/// The value is never serialized on the wire (it is a purely client-side
/// bookkeeping token) — do not confuse it with the protocol *circuit code*. The
/// default/zero value is the "no circuit / unknown" sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CircuitId(pub u64);

impl CircuitId {
    /// Wraps a raw circuit-instance number.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The raw circuit-instance number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for CircuitId {
    /// Formats as `circuit#<n>`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "circuit#{}", self.0)
    }
}

/// A [`RegionLocalObjectId`] paired with the [`CircuitId`] of the circuit it was
/// learned on, so it can only be acted upon against that same circuit instance.
///
/// The [`Session`](crate::Session) surfaces scoped object ids on the
/// [`Event`](crate::Event)s that hand an object id back to the caller and via
/// [`Object::scoped_id`](crate::Object::scoped_id); the object `Session` methods
/// consume them, resolving the circuit and erroring with
/// [`Error::UnknownCircuit`](crate::Error::UnknownCircuit) if it has gone away
/// (a stale id). The wire codec only ever sees the bare
/// [`RegionLocalObjectId`]; the scope is a client-side, never-serialized concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopedObjectId {
    /// The circuit instance the [`id`](Self::id) is valid on.
    pub circuit: CircuitId,
    /// The region-local object id, valid only on [`circuit`](Self::circuit).
    pub id: RegionLocalObjectId,
}

impl ScopedObjectId {
    /// Pairs a region-local object id with the circuit it belongs to.
    #[must_use]
    pub const fn new(circuit: CircuitId, id: RegionLocalObjectId) -> Self {
        Self { circuit, id }
    }

    /// The circuit instance the id is valid on.
    #[must_use]
    pub const fn circuit(self) -> CircuitId {
        self.circuit
    }

    /// The bare region-local object id.
    #[must_use]
    pub const fn id(self) -> RegionLocalObjectId {
        self.id
    }
}

impl std::fmt::Display for ScopedObjectId {
    /// Formats as `<circuit>/<id>`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.circuit, self.id)
    }
}

/// A [`RegionLocalParcelId`] paired with the [`CircuitId`] of the circuit it was
/// learned on, so it can only be acted upon against that same circuit instance.
///
/// The parcel sibling of [`ScopedObjectId`]: surfaced on the parcel
/// [`Event`](crate::Event)s that hand a parcel id back, consumed by the parcel
/// `Session` methods (which resolve the circuit and error with
/// [`Error::UnknownCircuit`](crate::Error::UnknownCircuit) when it is gone). The
/// wire codec only ever sees the bare [`RegionLocalParcelId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopedParcelId {
    /// The circuit instance the [`id`](Self::id) is valid on.
    pub circuit: CircuitId,
    /// The region-local parcel id, valid only on [`circuit`](Self::circuit).
    pub id: RegionLocalParcelId,
}

impl ScopedParcelId {
    /// Pairs a region-local parcel id with the circuit it belongs to.
    #[must_use]
    pub const fn new(circuit: CircuitId, id: RegionLocalParcelId) -> Self {
        Self { circuit, id }
    }

    /// The circuit instance the id is valid on.
    #[must_use]
    pub const fn circuit(self) -> CircuitId {
        self.circuit
    }

    /// The bare region-local parcel id.
    #[must_use]
    pub const fn id(self) -> RegionLocalParcelId {
        self.id
    }
}

impl std::fmt::Display for ScopedParcelId {
    /// Formats as `<circuit>/<id>`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.circuit, self.id)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_wire::{RegionLocalObjectId, RegionLocalParcelId};

    use super::{CircuitId, ScopedObjectId, ScopedParcelId};

    #[test]
    fn circuit_id_round_trips() {
        let id = CircuitId::new(42);
        assert_eq!(id.get(), 42);
        assert_eq!(CircuitId::default(), CircuitId(0));
    }

    #[test]
    fn scoped_object_id_accessors() {
        let scoped = ScopedObjectId::new(CircuitId(3), RegionLocalObjectId(7));
        assert_eq!(scoped.circuit(), CircuitId(3));
        assert_eq!(scoped.id(), RegionLocalObjectId(7));
        assert_eq!(
            scoped,
            ScopedObjectId::new(CircuitId(3), RegionLocalObjectId(7))
        );
    }

    #[test]
    fn scoped_parcel_id_accessors() {
        let scoped = ScopedParcelId::new(CircuitId(5), RegionLocalParcelId(-1));
        assert_eq!(scoped.circuit(), CircuitId(5));
        assert_eq!(scoped.id(), RegionLocalParcelId(-1));
    }

    #[test]
    fn same_id_different_circuit_is_unequal() {
        // The whole point: the same region-local id on two circuit instances is
        // two distinct scoped ids (a reconnect mints a fresh circuit).
        let a = ScopedObjectId::new(CircuitId(1), RegionLocalObjectId(100));
        let b = ScopedObjectId::new(CircuitId(2), RegionLocalObjectId(100));
        assert_ne!(a, b);
    }

    #[test]
    fn display_formats() {
        assert_eq!(CircuitId(9).to_string(), "circuit#9");
        assert_eq!(
            ScopedObjectId::new(CircuitId(2), RegionLocalObjectId(7)).to_string(),
            "circuit#2/7"
        );
    }
}
