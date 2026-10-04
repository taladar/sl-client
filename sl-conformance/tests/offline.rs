//! The conformance cases that need no grid anyone has to stand up.
//!
//! One test per name in [`sl_conformance::fake::OFFLINE_CASES`] **and per fake
//! flavour it declares** — `<case>::fake_sl` and `<case>::fake_opensim` — each
//! starting its own [`sl_fake_grid`] on ephemeral ports, running the registered
//! case body against it and logging out. A case here is exercised on every
//! `cargo test` — and therefore on every commit — rather than the next time
//! somebody remembers to log a live grid in.
//!
//! A case gets its own test per flavour rather than sharing a loop so a failure
//! names the case and the grid, and its own grid rather than sharing one so a
//! case that mutates the region cannot decide what the next one sees.
//!
//! Nothing here writes a record. The committed `records/` tree is for runs
//! against a grid that has to be logged into by hand, where the last known
//! answer is worth keeping; this answer is re-made from scratch every time the
//! suite runs, so a stored copy of it could only ever be staler than the truth.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_conformance::Grid;
    use sl_conformance::fake::{OFFLINE_CASES, run_offline_case};

    /// What a test returns when the case, or the lookup that found it, failed.
    type TestError = String;

    /// Run the registered case called `name` against a fresh fake grid of
    /// `flavour`.
    async fn offline(name: &str, flavour: Grid) -> Result<(), TestError> {
        let test =
            sl_conformance::find(name).ok_or_else(|| format!("{name} is not in the registry"))?;
        run_offline_case(test.as_ref(), flavour)
            .await
            .map_err(|failure| format!("{name} on {flavour}: {failure}"))
    }

    /// The [`Grid`] a flavour's test name stands for.
    macro_rules! flavour {
        (fake_sl) => {
            Grid::FakeSl
        };
        (fake_opensim) => {
            Grid::FakeOpensim
        };
    }

    /// Declares a module per case holding one `#[tokio::test]` per flavour, and
    /// the roll-call test that proves the set of them is exactly what
    /// [`OFFLINE_CASES`] and the cases' own `grids()` declare.
    ///
    /// The pairing is what makes the list trustworthy: a case (or a flavour of
    /// one) declared in the library but not here would be believed to run and
    /// never would.
    macro_rules! offline_cases {
        ($($module:ident => $case:literal [$($flavour:ident),+],)+) => {
            $(
                /// The offline runs of one case, one test per flavour.
                mod $module {
                    use super::{Grid, TestError, offline};

                    $(
                        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
                        async fn $flavour() -> Result<(), TestError> {
                            offline($case, flavour!($flavour)).await
                        }
                    )+
                }
            )+

            /// Every `(case, flavour)` this file declares a test for, for the
            /// roll call below.
            const DECLARED: &[(&str, Grid)] = &[$($(($case, flavour!($flavour)),)+)+];
        };
    }

    offline_cases! {
        login_handshake => "login-handshake" [fake_sl, fake_opensim],
        keepalive_ping => "keepalive-ping" [fake_sl, fake_opensim],
        throttle_set => "throttle-set" [fake_sl, fake_opensim],
        simulator_features => "simulator-features" [fake_sl, fake_opensim],
        object_update_decode => "object-update-decode" [fake_sl, fake_opensim],
        parcel_properties => "parcel-properties" [fake_sl, fake_opensim],
        terrain_raw_transfer_download => "terrain-raw-transfer-download" [fake_sl, fake_opensim],
        terrain_layerdata => "terrain-layerdata" [fake_sl, fake_opensim],
        map_blocks_items => "map-blocks-items" [fake_sl, fake_opensim],
        teleport_local_phases => "teleport-local-phases" [fake_sl, fake_opensim],
        teleport_cross_region => "teleport-cross-region" [fake_sl, fake_opensim],
        region_crossing => "region-crossing" [fake_sl, fake_opensim],
        neighbour_child_circuits => "neighbour-child-circuits" [fake_sl, fake_opensim],
        avatar_appearance_npc => "avatar-appearance-npc" [fake_sl, fake_opensim],
        server_appearance_bake => "server-appearance-bake" [fake_sl, fake_opensim],
        texture_fetch_http => "texture-fetch-http" [fake_sl, fake_opensim],
        asset_fetch_http => "asset-fetch-http" [fake_sl, fake_opensim],
        economy_data => "economy-data" [fake_sl, fake_opensim],
        parcel_info_dwell => "parcel-info-dwell" [fake_sl, fake_opensim],
        agent_alert => "agent-alert" [fake_sl, fake_opensim],
        chat_self_echo => "chat-self-echo" [fake_sl, fake_opensim],
        chat_hear_other => "chat-hear-other" [fake_sl, fake_opensim],
        chat_whisper_shout_range => "chat-whisper-shout-range" [fake_sl, fake_opensim],
        server_error => "server-error" [fake_sl, fake_opensim],
        task_inventory => "task-inventory" [fake_sl, fake_opensim],
        asset_round_trip => "asset-round-trip" [fake_opensim],
        object_asset_format => "object-asset-format" [fake_sl, fake_opensim],
        object_edit => "object-edit" [fake_sl, fake_opensim],
        object_link_delink => "object-link-delink" [fake_sl, fake_opensim],
        object_properties => "object-properties" [fake_sl, fake_opensim],
        object_rez_derez => "object-rez-derez" [fake_sl, fake_opensim],
        parcel_edit => "parcel-edit" [fake_sl, fake_opensim],
        region_info => "region-info" [fake_sl, fake_opensim],
        estate_info => "estate-info" [fake_sl, fake_opensim],
        estate_access => "estate-access" [fake_sl, fake_opensim],
        logout_clean => "logout-clean" [fake_sl, fake_opensim],
    }

    /// The tests declared above are exactly the offline cases, each on exactly
    /// the fake flavours it declares.
    ///
    /// Both halves matter: a `(case, flavour)` the library declares with no
    /// test here is a run nobody makes while the crate claims otherwise, and a
    /// test here the case does not declare would be refused by the runner.
    #[test]
    fn every_offline_case_has_a_test_per_flavour() -> Result<(), TestError> {
        let mut declared: Vec<(&str, &str)> = DECLARED
            .iter()
            .map(|(case, flavour)| (*case, flavour.dir_name()))
            .collect();
        let mut listed: Vec<(&str, &str)> = Vec::new();
        for name in OFFLINE_CASES {
            let test =
                sl_conformance::find(name).ok_or_else(|| format!("{name} is not registered"))?;
            for grid in test.grids().iter().filter(|grid| grid.is_fake()) {
                listed.push((*name, grid.dir_name()));
            }
        }
        declared.sort_unstable();
        listed.sort_unstable();
        assert_eq!(
            declared, listed,
            "the tests in this file and the offline cases' flavours have drifted apart"
        );
        Ok(())
    }
}
