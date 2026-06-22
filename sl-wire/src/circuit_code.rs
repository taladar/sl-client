//! The circuit code newtype.
//!
//! The *circuit code* is a `u32` the login server assigns to an agent's session
//! and the client echoes back in `UseCircuitCode` to authenticate a circuit to a
//! simulator. Unlike a per-connection token, one circuit code is **reused across
//! every circuit** of a single agent session (the root circuit at login and each
//! child circuit to a neighbouring region all send the same code). It therefore
//! identifies *which agent session* a datagram belongs to, not which individual
//! connection — that local distinction is the separate `CircuitId` minted in
//! `sl-proto`.
//!
//! Because the raw `u32` carries this protocol meaning the compiler can't
//! otherwise see, it lives here as a newtype — mirroring
//! [`RegionHandle`](crate::RegionHandle) and the `sl-types` key wrappers — so a
//! circuit code can't be transposed with an unrelated 32-bit field.

/// The `u32` circuit code the login server assigns to an agent session and the
/// client sends in `UseCircuitCode` to authenticate each circuit (the reference
/// viewer's `LLHost`/`LLCircuitData` circuit code).
///
/// The same value is reused by every circuit of one agent session (root and all
/// children), so it scopes a datagram to the *session*, not to an individual
/// connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct CircuitCode(pub u32);

impl CircuitCode {
    /// Builds a circuit code from its raw `u32` wire value.
    #[must_use]
    pub const fn new(code: u32) -> Self {
        Self(code)
    }

    /// Returns the raw `u32` wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl core::fmt::Display for CircuitCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::CircuitCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn round_trips_raw_value() {
        let code = CircuitCode::new(123_456);
        assert_eq!(code.get(), 123_456);
        assert_eq!(CircuitCode(code.get()), code);
        assert_eq!(code.to_string(), "123456");
    }

    #[test]
    fn default_is_zero() {
        assert_eq!(CircuitCode::default(), CircuitCode(0));
    }
}
