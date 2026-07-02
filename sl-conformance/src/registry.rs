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

    /// Whether the primary session should run with the inventory disk cache
    /// enabled (default `false`). The runner then supplies a cleared per-case
    /// cache directory so the case starts cold and can observe the cache being
    /// reused across a relogin (the `inventory-cache-skip` case).
    fn inventory_cache(&self) -> bool {
        false
    }

    /// The `start` location every avatar of this test logs in at, as the wire
    /// string a grid expects (`"last"`, `"home"`, or `"uri:Region&x&y&z"`).
    ///
    /// Defaults to `"last"` (resume where the avatar logged out), which is right
    /// for almost every case. A case overrides it when it must be co-located
    /// with a fixed in-world resource — e.g. the Phase 8/9 object and scripting
    /// cases, whose rezzed OAR object lives in the OpenSim "Default Region", so
    /// they force a login there rather than trusting the avatar's last position.
    /// The `grid` is passed so an override can be OpenSim-specific (a named
    /// OpenSim region is meaningless on Second Life, where `"last"` is kept).
    fn start_location(&self, _grid: Grid) -> &'static str {
        "last"
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
        Box::new(crate::cases::inventory_tree_crawl::InventoryTreeCrawl),
        Box::new(crate::cases::library_tree_fetch::LibraryTreeFetch),
        Box::new(crate::cases::inventory_item_ops::InventoryItemOps),
        Box::new(crate::cases::inventory_cache_skip::InventoryCacheSkip),
        Box::new(crate::cases::give_inventory::GiveInventory),
        Box::new(crate::cases::ais3_folder_lifecycle::Ais3FolderLifecycle),
        Box::new(crate::cases::asset_decode::AssetDecode),
        Box::new(crate::cases::avatar_properties::AvatarProperties),
        Box::new(crate::cases::avatar_notes::AvatarNotes),
        Box::new(crate::cases::profile_edit_roundtrip::ProfileEditRoundtrip),
        Box::new(crate::cases::picks_classifieds::PicksClassifieds),
        Box::new(crate::cases::display_names::DisplayNames),
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
        Box::new(crate::cases::group_session_message::GroupSessionMessage),
        Box::new(crate::cases::group_accounting::GroupAccounting),
        Box::new(crate::cases::group_create_activate::GroupCreateActivate),
        Box::new(crate::cases::group_join_leave::GroupJoinLeave),
        Box::new(crate::cases::group_notice::GroupNotice),
        Box::new(crate::cases::group_roster::GroupRoster),
        Box::new(crate::cases::group_admin::GroupAdmin),
        Box::new(crate::cases::chat_invite_accept_decline::ChatInviteAcceptDecline),
        Box::new(crate::cases::session_mark_read::SessionMarkRead),
        Box::new(crate::cases::offline_msg_fetch::OfflineMsgFetch),
        Box::new(crate::cases::friendship_offer_accept::FriendshipOfferAccept),
        Box::new(crate::cases::friendship_terminate::FriendshipTerminate),
        Box::new(crate::cases::presence_online_offline::PresenceOnlineOffline),
        Box::new(crate::cases::grant_user_rights::GrantUserRights),
        Box::new(crate::cases::calling_card::CallingCard),
        Box::new(crate::cases::mute_list::MuteList),
        Box::new(crate::cases::object_update_decode::ObjectUpdateDecode),
        Box::new(crate::cases::object_properties::ObjectProperties),
        Box::new(crate::cases::object_rez_derez::ObjectRezDerez),
        Box::new(crate::cases::object_touch_grab::ObjectTouchGrab),
        Box::new(crate::cases::object_link_delink::ObjectLinkDelink),
        Box::new(crate::cases::object_edit::ObjectEdit),
        Box::new(crate::cases::task_inventory::TaskInventory),
        Box::new(crate::cases::script_dialog::ScriptDialog),
        Box::new(crate::cases::script_permissions::ScriptPermissionsCase),
        Box::new(crate::cases::script_running::ScriptRunning),
        Box::new(crate::cases::script_upload::ScriptUpload),
        Box::new(crate::cases::parcel_properties::ParcelProperties),
        Box::new(crate::cases::parcel_info_dwell::ParcelInfoDwell),
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
