//! The [`GridTest`] trait and the curated registry of conformance tests.
//!
//! A test names itself, declares the grids it is meaningful on and how many
//! avatars it needs, and exposes an async body that drives a [`TestContext`].
//! The runner looks tests up by name; there is deliberately no facility to run
//! them all at once.

use crate::context::TestContext;
pub use crate::context::TestFailure;
use crate::grid::Grid;

/// The boxed future returned by a test body.
pub type TestFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), TestFailure>> + Send + 'a>>;

/// One conformance test: a named, grid-scoped feature exercise.
pub trait GridTest: Send + Sync {
    /// The stable kebab-case identifier; also the record file stem.
    fn name(&self) -> &'static str;

    /// A one-line human description, shown by `list`.
    fn description(&self) -> &'static str;

    /// The grids on which this test is meaningful.
    fn grids(&self) -> &'static [Grid];

    /// How many distinct logged-in avatars the test needs (1, 2, or 3).
    fn accounts(&self) -> u8 {
        1
    }

    /// Run the exercise against the (already logged-in) context.
    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a>;
}

/// The curated set of conformance tests, in display order.
#[must_use]
pub fn registry() -> Vec<Box<dyn GridTest>> {
    vec![
        Box::new(crate::cases::login_handshake::LoginHandshake),
        Box::new(crate::cases::inventory_fetch::InventoryFetch),
        Box::new(crate::cases::asset_decode::AssetDecode),
        Box::new(crate::cases::region_info::RegionInfo),
        Box::new(crate::cases::logout_clean::LogoutClean),
        Box::new(crate::cases::keepalive_ping::KeepalivePing),
        Box::new(crate::cases::throttle_set::ThrottleSet),
        Box::new(crate::cases::draw_distance::DrawDistance),
        Box::new(crate::cases::chat_self_echo::ChatSelfEcho),
        Box::new(crate::cases::chat_hear_other::ChatHearOther),
        Box::new(crate::cases::chat_whisper_shout_range::ChatWhisperShoutRange),
        Box::new(crate::cases::typing_indicator::TypingIndicator),
        Box::new(crate::cases::im_1to1::Im1to1),
        Box::new(crate::cases::im_typing::ImTyping),
    ]
}

/// Find a registered test by name.
#[must_use]
pub fn find(name: &str) -> Option<Box<dyn GridTest>> {
    registry().into_iter().find(|test| test.name() == name)
}

#[cfg(test)]
mod tests {
    use super::{find, registry};
    use pretty_assertions::assert_eq;

    /// Every registered test has a unique name and at least one grid.
    #[test]
    fn registry_is_well_formed() {
        let tests = registry();
        let mut names: Vec<&str> = tests.iter().map(|test| test.name()).collect();
        names.sort_unstable();
        let unique = {
            let mut copy = names.clone();
            copy.dedup();
            copy.len()
        };
        assert_eq!(unique, names.len(), "test names must be unique");
        for test in &tests {
            assert!(
                !test.grids().is_empty(),
                "{} must apply to at least one grid",
                test.name()
            );
        }
        assert!(find("inventory-fetch").is_some());
        assert!(find("does-not-exist").is_none());
    }
}
