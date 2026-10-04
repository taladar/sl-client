//! Which capabilities each grid's seed grants, and to whom.
//!
//! A region's seed capability answers a list of names with the subset it
//! grants. That subset decides which protocol path a viewer takes for almost
//! everything — an HTTP capability where the grid offers one, a UDP message or
//! nothing where it does not — and it is where Second Life and OpenSim differ
//! most visibly.
//!
//! The case records, on the start region:
//!
//! - what the client's own list ([`REQUESTED_CAPABILITIES`]) gets granted, and
//!   which of its names are refused (`granted_ours`, `refused_ours`);
//! - after a relogin asking for the reference viewer's list as well
//!   ([`REFERENCE_CAPABILITIES`]), which names only the reference asks for are
//!   granted (`granted_reference_only`) and which are refused;
//! - each neighbour region's grant, where the grid opens a child circuit
//!   (`neighbour_granted`, and what differs from the root region's).
//!
//! `1av`, `[both, fake]`. On aditi the relogin waits out the login cooldown.
//! The measured sets are in `book/src/gridspec/capabilities.md`.

use std::collections::BTreeSet;
use std::time::Duration;

use sl_client_tokio::REQUESTED_CAPABILITIES;

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::REGION_TIMEOUT;

/// The capability names the reference viewer asks every seed for: Firestorm's
/// `LLViewerRegionImpl::buildCapabilityNames` (`llviewerregion.cpp`).
pub const REFERENCE_CAPABILITIES: &[&str] = &[
    "AbuseCategories",
    "AcceptFriendship",
    "AcceptGroupInvite",
    "AgentExperiences",
    "AgentPreferences",
    "AgentProfile",
    "AgentState",
    "AttachmentResources",
    "AvatarPickerSearch",
    "AvatarRenderInfo",
    "CharacterProperties",
    "ChatSessionRequest",
    "CopyInventoryFromNotecard",
    "CreateInventoryCategory",
    "DeclineFriendship",
    "DeclineGroupInvite",
    "DirectDelivery",
    "DispatchOpenRegionSettings",
    "DispatchRegionInfo",
    "EnvironmentSettings",
    "EstateAccess",
    "EstateChangeInfo",
    "EventQueueGet",
    "ExperiencePreferences",
    "ExperienceQuery",
    "ExtEnvironment",
    "FetchInventory2",
    "FetchInventoryDescendents2",
    "FetchLib2",
    "FetchLibDescendents2",
    "FindExperienceByName",
    "GetAdminExperiences",
    "GetCreatorExperiences",
    "GetDisplayNames",
    "GetExperienceInfo",
    "GetExperiences",
    "GetMesh",
    "GetMesh2",
    "GetMetadata",
    "GetObjectCost",
    "GetObjectPhysicsData",
    "GetTexture",
    "GroupAPIv1",
    "GroupExperiences",
    "GroupMemberData",
    "GroupProposalBallot",
    "HomeLocation",
    "IncrementCOFVersion",
    "InterestList",
    "InventoryThumbnailUpload",
    "IsExperienceAdmin",
    "IsExperienceContributor",
    "LandResources",
    "LSLSyntax",
    "MapLayer",
    "MapLayerGod",
    "MeshUploadFlag",
    "ModifyMaterialParams",
    "ModifyRegion",
    "NavMeshGenerationStatus",
    "NewFileAgentInventory",
    "ObjectAnimation",
    "ObjectMedia",
    "ObjectMediaNavigate",
    "ObjectNavMeshProperties",
    "ParcelPropertiesUpdate",
    "ParcelVoiceInfoRequest",
    "ProductInfoRequest",
    "ProvisionVoiceAccountRequest",
    "ReadOfflineMsgs",
    "RegionExperiences",
    "RegionObjects",
    "RegionSchedule",
    "RemoteParcelRequest",
    "RenderMaterials",
    "RequestTaskInventory",
    "RequestTextureDownload",
    "ResourceCostSelected",
    "RetrieveNavMeshSrc",
    "SearchStatRequest",
    "SearchStatTracking",
    "SendPostcard",
    "SendUserReport",
    "SendUserReportWithScreenshot",
    "ServerReleaseNotes",
    "SetDisplayName",
    "SimConsoleAsync",
    "SimulatorFeatures",
    "SpatialVoiceModerationRequest",
    "StartGroupProposal",
    "TerrainNavMeshProperties",
    "TextureStats",
    "UntrustedSimulatorMessage",
    "UpdateAgentInformation",
    "UpdateAgentLanguage",
    "UpdateAvatarAppearance",
    "UpdateExperience",
    "UpdateGestureAgentInventory",
    "UpdateGestureTaskInventory",
    "UpdateMaterialAgentInventory",
    "UpdateMaterialTaskInventory",
    "UpdateNotecardAgentInventory",
    "UpdateNotecardTaskInventory",
    "UpdateScriptAgent",
    "UpdateScriptTask",
    "UpdateSettingsAgentInventory",
    "UpdateSettingsTaskInventory",
    "UploadAgentProfileImage",
    "UploadBakedTexture",
    "UserInfo",
    "ViewerAsset",
    "ViewerBenefits",
    "ViewerMetrics",
    "ViewerStartAuction",
    "ViewerStats",
    "VoiceSignalingRequest",
];

/// What each grid's seed refused of the client's own list, as the imitated
/// grids record it ([`sl_fake_grid::ImitatedGrid::withheld_capabilities`]).
const REFUSED_OURS: Measured<&[&str]> = Measured {
    second_life: sl_fake_grid::imitates::SECOND_LIFE_REFUSED_CAPABILITIES,
    opensim: sl_fake_grid::imitates::OPENSIM_REFUSED_CAPABILITIES,
    source: "book/src/gridspec/capabilities.md (seed-capabilities, 2026-10-04)",
};

/// Hold `refused` to the measurement: on a live grid exactly the measured
/// set; on a fake one **at least** it — a fake grid may refuse more, because
/// it does not implement every capability a live grid offers, but it must not
/// grant one its live grid refuses, or a client takes a path it could not
/// take there.
fn check_refused(grid: Grid, refused: &BTreeSet<String>) -> Result<(), TestFailure> {
    let measured: BTreeSet<String> = set_of(REFUSED_OURS.on(grid));
    if grid.is_fake() {
        let granted_anyway: BTreeSet<String> = measured.difference(refused).cloned().collect();
        if granted_anyway.is_empty() {
            return Ok(());
        }
        return Err(TestFailure::Assertion(format!(
            "{grid} grants {} which the grid it imitates refuses ({})",
            joined(&granted_anyway),
            REFUSED_OURS.source
        )));
    }
    REFUSED_OURS.check(
        "the capabilities the seed refused",
        grid,
        &refused
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice(),
    )
}

/// How long to give neighbour regions' seeds to answer after arrival.
const NEIGHBOUR_WAIT: Duration = Duration::from_secs(20);

/// The overall budget: two logins, one behind aditi's cooldown.
const CASE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// `names` as one comma-separated metric value.
fn joined(names: &BTreeSet<String>) -> String {
    names.iter().cloned().collect::<Vec<_>>().join(",")
}

/// `names` as an owned set.
fn set_of(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|&name| name.to_owned()).collect()
}

/// The capability names the root region granted, once it has granted any.
async fn granted(session: &mut Session) -> Result<BTreeSet<String>, TestFailure> {
    session.wait_for_region(REGION_TIMEOUT).await?;
    let names = session.capability_names();
    if names.is_empty() {
        return Err(TestFailure::State(
            "the region granted no capabilities at all".to_owned(),
        ));
    }
    Ok(names)
}

/// Surveys which capabilities each grid's seed grants.
#[derive(Debug)]
pub struct SeedCapabilities;

impl GridTest for SeedCapabilities {
    fn name(&self) -> &'static str {
        "seed-capabilities"
    }

    fn description(&self) -> &'static str {
        "Record which capabilities the seed grants, for our list and the reference viewer's"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let ours = set_of(REQUESTED_CAPABILITIES);
            let reference = set_of(REFERENCE_CAPABILITIES);
            let everything: BTreeSet<String> = ours.union(&reference).cloned().collect();
            let session = ctx.primary();
            let granted_default = granted(session).await?;
            let granted_ours: BTreeSet<String> =
                granted_default.intersection(&ours).cloned().collect();
            let refused_ours: BTreeSet<String> =
                ours.difference(&granted_default).cloned().collect();

            session.disconnect().await?;
            session
                .relogin_requesting_capabilities(Some(everything.iter().cloned().collect()))
                .await?;
            let granted_all = granted(session).await?;
            tokio::time::sleep(NEIGHBOUR_WAIT).await;
            let neighbours = session.neighbour_capability_names();
            let reference_only: BTreeSet<String> = reference.difference(&ours).cloned().collect();
            let granted_reference_only: BTreeSet<String> =
                granted_all.intersection(&reference_only).cloned().collect();
            let refused_reference_only: BTreeSet<String> =
                reference_only.difference(&granted_all).cloned().collect();
            let granted_unasked: BTreeSet<String> =
                granted_all.difference(&everything).cloned().collect();
            // Back to the client's own list for the runner's logout.
            session.disconnect().await?;
            session.relogin_requesting_capabilities(None).await?;
            session.wait_for_region(REGION_TIMEOUT).await?;

            let metrics = ctx.metrics();
            metrics.set("granted_ours", joined(&granted_ours));
            metrics.set("refused_ours", joined(&refused_ours));
            metrics.set("granted_reference_only", joined(&granted_reference_only));
            metrics.set("refused_reference_only", joined(&refused_reference_only));
            metrics.set("granted_unasked", joined(&granted_unasked));
            metrics.set(
                "neighbour_count",
                i64::try_from(neighbours.len()).unwrap_or(-1),
            );
            check_refused(grid, &refused_ours)?;
            if let Some((_sim, neighbour)) = neighbours.first() {
                let root_only: BTreeSet<String> =
                    granted_all.difference(neighbour).cloned().collect();
                let neighbour_only: BTreeSet<String> =
                    neighbour.difference(&granted_all).cloned().collect();
                metrics.set("neighbour_granted", joined(neighbour));
                metrics.set("root_only", joined(&root_only));
                metrics.set("neighbour_only", joined(&neighbour_only));
            }
            Ok(())
        })
    }
}
