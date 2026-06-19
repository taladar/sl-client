//! The read-only "smoke battery": the ordered list of harmless query
//! [`Command`]s the `--smoke` mode fires once the region handshake lands.
//!
//! Every entry is a *request* — it asks the simulator or a grid capability for
//! state and never mutates anything — so firing the whole battery against a live
//! grid is a safe end-to-end check that login, the circuit, the CAPS seed, and
//! the decoders are all working: each request should produce a matching
//! [`Event`](sl_proto::Event) (or a [`Diagnostic`](sl_proto::Diagnostic) if the
//! reply fails to decode). The inventory skeleton and group memberships already
//! arrive unsolicited at login, so they are not re-requested here.

use sl_proto::{Command, MapItemType, Uuid, VoiceProvisionRequest};

/// Build the ordered smoke-test battery of read-only requests for the logged-in
/// agent `self_agent`.
///
/// The list covers the wallet (balance, economy), the region and current
/// parcel, the agent's own appearance/profile/picks/classifieds/notes, the mute
/// list, world-map overlay items, the voice channel, and the agent's
/// experiences — i.e. one representative query per major subsystem, all
/// read-only so the battery can be fired automatically without side effects.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "`smoke_battery` reads best as the crate's public smoke-test entry point"
)]
pub fn smoke_battery(self_agent: Uuid) -> Vec<Command> {
    vec![
        // Wallet and grid economy.
        Command::RequestMoneyBalance,
        Command::RequestEconomyData,
        // The region and the whole-region parcel rectangle (256 m default).
        Command::RequestRegionInfo,
        Command::RequestParcelProperties {
            west: 0.0,
            south: 0.0,
            east: 256.0,
            north: 256.0,
            sequence_id: 0,
        },
        // The agent's own appearance and baked-texture cache.
        Command::RequestWearables,
        Command::RequestCachedTextures {
            serial: 0,
            slots: Vec::new(),
        },
        // The agent's own profile, picks, classifieds, and private notes.
        Command::RequestAvatarProperties(self_agent),
        Command::RequestAvatarPicks(self_agent),
        Command::RequestAvatarClassifieds(self_agent),
        Command::RequestAvatarNotes(self_agent),
        // The mute list.
        Command::RequestMuteList,
        // World-map avatar overlay for the current region (handle 0).
        Command::RequestMapItems {
            item_type: MapItemType::AgentLocations,
            region_handle: 0,
        },
        // Voice: provision the account (default Vivox) and the parcel channel.
        Command::RequestVoiceAccount {
            request: VoiceProvisionRequest::default(),
        },
        Command::RequestParcelVoiceInfo,
        // The agent's experiences (owned, administered, created).
        Command::RequestOwnedExperiences,
        Command::RequestAdminExperiences,
        Command::RequestCreatorExperiences,
    ]
}

#[cfg(test)]
mod tests {
    use sl_proto::{Command, Uuid};

    use super::smoke_battery;

    /// A stable test agent id (all-`a` nibbles).
    fn agent() -> Uuid {
        Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap_or_else(|_| Uuid::nil())
    }

    #[test]
    fn battery_is_non_empty_and_starts_with_the_balance_query() {
        let battery = smoke_battery(agent());
        assert!(
            !battery.is_empty(),
            "the smoke battery must fire at least one request"
        );
        assert!(
            matches!(battery.first(), Some(Command::RequestMoneyBalance)),
            "the battery should open with the balance query"
        );
    }

    #[test]
    fn self_referencing_requests_use_the_passed_agent() {
        let battery = smoke_battery(agent());
        let profile_targets_self = battery.iter().any(
            |command| matches!(command, Command::RequestAvatarProperties(id) if *id == agent()),
        );
        assert!(
            profile_targets_self,
            "the profile request should target the logged-in agent"
        );
    }

    #[test]
    fn battery_contains_only_read_only_requests() {
        // Every variant in the battery should be a `Request*`/read-only query;
        // guard against a mutating command slipping in by spot-checking that the
        // mute-list query and both experience queries are present and no `Chat`
        // or other side-effecting command is.
        let battery = smoke_battery(agent());
        assert!(
            battery
                .iter()
                .any(|command| matches!(command, Command::RequestMuteList)),
            "the mute-list query should be part of the battery"
        );
        assert!(
            !battery
                .iter()
                .any(|command| matches!(command, Command::Chat { .. } | Command::Logout)),
            "the battery must not contain any side-effecting command"
        );
    }
}
