//! The conformance cases that need no grid anyone has to stand up.
//!
//! One test per name in [`sl_conformance::fake::OFFLINE_CASES`], each starting
//! its own [`sl_fake_grid`] on ephemeral ports, running the registered case body
//! against it and logging out. A case here is exercised on every `cargo test` —
//! and therefore on every commit — rather than the next time somebody remembers
//! to log a live grid in.
//!
//! A case gets its own test rather than sharing a loop so a failure names the
//! case, and its own grid rather than sharing one so a case that mutates the
//! region cannot decide what the next one sees.
//!
//! Nothing here writes a record. The committed `records/` tree is for runs
//! against a grid that has to be logged into by hand, where the last known
//! answer is worth keeping; this answer is re-made from scratch every time the
//! suite runs, so a stored copy of it could only ever be staler than the truth.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_conformance::fake::{OFFLINE_CASES, run_offline_case};

    /// What a test returns when the case, or the lookup that found it, failed.
    type TestError = String;

    /// Run the registered case called `name` against a fresh fake grid.
    async fn offline(name: &str) -> Result<(), TestError> {
        let test =
            sl_conformance::find(name).ok_or_else(|| format!("{name} is not in the registry"))?;
        run_offline_case(test.as_ref())
            .await
            .map_err(|failure| format!("{name}: {failure}"))
    }

    /// Declares one `#[tokio::test]` per case name, and the roll-call test that
    /// proves the set of them is exactly [`OFFLINE_CASES`].
    ///
    /// The pairing is what makes the list trustworthy: a case added to
    /// `OFFLINE_CASES` but not here would be believed to run and never would.
    macro_rules! offline_cases {
        ($($test_name:ident => $case:literal,)+) => {
            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
                async fn $test_name() -> Result<(), TestError> {
                    offline($case).await
                }
            )+

            /// Every case this file declares a test for, for the roll call below.
            const DECLARED: &[&str] = &[$($case,)+];
        };
    }

    offline_cases! {
        login_handshake => "login-handshake",
        keepalive_ping => "keepalive-ping",
        throttle_set => "throttle-set",
        simulator_features => "simulator-features",
        object_update_decode => "object-update-decode",
        parcel_properties => "parcel-properties",
        terrain_raw_transfer_download => "terrain-raw-transfer-download",
        terrain_layerdata => "terrain-layerdata",
        map_blocks_items => "map-blocks-items",
        teleport_local_phases => "teleport-local-phases",
        teleport_cross_region => "teleport-cross-region",
        region_crossing => "region-crossing",
        neighbour_child_circuits => "neighbour-child-circuits",
        avatar_appearance_npc => "avatar-appearance-npc",
        texture_fetch_http => "texture-fetch-http",
        logout_clean => "logout-clean",
    }

    /// The tests declared above are exactly the cases the library calls offline.
    ///
    /// Both halves matter: a name in [`OFFLINE_CASES`] with no test here is a
    /// case nobody runs while the crate claims otherwise, and a test here for a
    /// name not in the list runs a case whose `grids()` does not declare the
    /// fake grid — so the runner would refuse the very run this file just made.
    #[test]
    fn every_offline_case_has_a_test() {
        let mut declared: Vec<&str> = DECLARED.to_vec();
        let mut listed: Vec<&str> = OFFLINE_CASES.to_vec();
        declared.sort_unstable();
        listed.sort_unstable();
        assert_eq!(
            declared, listed,
            "the tests in this file and OFFLINE_CASES have drifted apart"
        );
    }
}
