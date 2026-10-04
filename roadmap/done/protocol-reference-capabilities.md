---
id: protocol-reference-capabilities
title: Request and use the capabilities the reference viewer asks for and the grids grant
topic: protocol
status: done
origin: gridspec-seed-capabilities (2026-10-04)
refs: [gridspec-seed-capabilities, viewer-display-name-set, viewer-social-group-extras,
  viewer-i18n-agent-language, protocol-avatar-render-info, gridspec-task-inventory,
  viewer-parcel-config-missing-writes]
---

Context: [context/protocol.md](../context/protocol.md).

## Done (2026-10-04): triaged into tasks

Each of the 51 names Firestorm requests and we did not was checked against
Firestorm's code, our client and OpenSim's, and either filed as work or
written down as skipped. The request and response shapes are in
`book/src/comms/caps-reference.md`; which grid grants what is in
`book/src/gridspec/capabilities.md`. The rule for every adoption is unchanged:
use the capability where the region grants it, keep the existing path where it
does not.

**A modern path for something we already do** (8):

| capability | task |
| --- | --- |
| `ParcelPropertiesUpdate` | [[protocol-cap-parcel-properties-update]] (also fixes [[parcel-properties-update-via-udp-poisons-opensim]] — do first) |
| `HomeLocation` | [[protocol-cap-home-location]] |
| `EstateAccess` | [[protocol-cap-estate-access]] |
| `DispatchRegionInfo` | [[protocol-cap-dispatch-region-info]] |
| `AgentProfile` | [[protocol-cap-agent-profile]] |
| `AcceptFriendship`, `DeclineFriendship` | [[protocol-cap-offline-friendship-answers]] |
| `UpdateAgentInformation` | [[protocol-cap-update-agent-information]] |
| `RequestTaskInventory` | [[protocol-request-task-inventory-cap]] |

**Missing features** (28):

| capability | task |
| --- | --- |
| `UploadAgentProfileImage` | [[viewer-profile-image-editing]] |
| `SetDisplayName` | [[viewer-display-name-set]] |
| `GroupAPIv1` | [[protocol-cap-group-bans]], [[viewer-social-group-extras]] |
| `UpdateAgentLanguage` | [[viewer-i18n-agent-language]] |
| `InterestList` | [[protocol-cap-interest-list]] |
| `AbuseCategories` | [[viewer-report-abuse]] |
| `AgentState`, `NavMeshGenerationStatus`, `RegionObjects`, `ObjectNavMeshProperties`, `TerrainNavMeshProperties`, `CharacterProperties` | [[viewer-pathfinding-floaters]] |
| `RetrieveNavMeshSrc` | [[viewer-pathfinding-navmesh-view]] |
| `UpdateGestureTaskInventory` | [[viewer-gesture-management-ui]] |
| `UpdateMaterialTaskInventory` | [[viewer-material-save-to-object]] |
| `InventoryThumbnailUpload` | [[viewer-inventory-thumbnails]] |
| `GetMetadata` | [[protocol-cap-get-metadata]] |
| `MeshUploadFlag` | [[viewer-mesh-upload-sequence]] |
| `SendPostcard` | [[viewer-snapshot-postcard]] |
| `ModifyRegion` | [[viewer-pbr-terrain]] |
| `RegionSchedule` | [[viewer-region-restart-schedule]] |
| `ProductInfoRequest` | [[protocol-cap-product-info]] |
| `ServerReleaseNotes` | [[viewer-about-release-notes]] |
| `SearchStatRequest` | [[viewer-classified-click-stats]] |
| `SpatialVoiceModerationRequest` | [[viewer-nearby-voice-moderation]] |
| `AvatarRenderInfo` | [[protocol-avatar-render-info]] |
| `UntrustedSimulatorMessage` (god kick / freeze) | [[viewer-god-tools]] |
| `SimConsoleAsync` | [[viewer-region-debug-console]] |

**Telemetry, opt-in** (4): `ViewerStats`, `ViewerMetrics`, `TextureStats`,
`SearchStatTracking` — [[viewer-telemetry-opt-in]]: off by default, a
preferences page saying exactly what each report sends.

**Not adopted, deprecated or unused** (11): `ViewerBenefits` and
`RequestTextureDownload` (requested by Firestorm, never called),
`StartGroupProposal` / `GroupProposalBallot` (group voting removed from Second
Life), `EnvironmentSettings` (pre-EEP; we use `ExtEnvironment`), `MapLayer` /
`MapLayerGod` (unused since viewer 2.0; the map uses tiles),
`DispatchOpenRegionSettings` (Aurora-Sim only), `EstateChangeInfo` (refused by
Second Life, disabled in OpenSim), `ViewerStartAuction` (refused; the god
auction goes over UDP).

## Before

[[gridspec-seed-capabilities]] asked both live grids for the reference
viewer's full capability list as well as ours
(`book/src/gridspec/capabilities.md`). Of the 51 names Firestorm requests and
we do not, **aditi grants 48 and OpenSim 12**. Each is a path where a grid
offers a capability and we take an older route — usually a UDP message the
capability replaced — or none at all.

Rule for every row: request the capability, use it where the region grants
it, and **keep the existing path** for a region that does not (the modern-CAPS
rule — the other grid family, or an older region, still needs it).

## Granted by both grids

`EnvironmentSettings`, `EstateAccess`, `HomeLocation`, `MapLayer`,
`MeshUploadFlag`, `ParcelPropertiesUpdate`, `ServerReleaseNotes`,
`SimConsoleAsync`, `UntrustedSimulatorMessage`, `UpdateAgentInformation`,
`UpdateAgentLanguage`, `UpdateGestureTaskInventory`.

## Granted by Second Life only

`AbuseCategories`, `AcceptFriendship`, `AgentProfile`, `AgentState`,
`AvatarRenderInfo`, `CharacterProperties`, `DeclineFriendship`,
`DispatchRegionInfo`, `GetMetadata`, `GroupAPIv1`, `GroupProposalBallot`,
`InterestList`, `InventoryThumbnailUpload`, `MapLayerGod`, `ModifyRegion`,
`NavMeshGenerationStatus`, `ObjectNavMeshProperties`, `ProductInfoRequest`,
`RegionObjects`, `RegionSchedule`, `RequestTaskInventory`,
`RequestTextureDownload`, `RetrieveNavMeshSrc`, `SearchStatRequest`,
`SearchStatTracking`, `SendPostcard`, `SetDisplayName`,
`SpatialVoiceModerationRequest`, `StartGroupProposal`,
`TerrainNavMeshProperties`, `TextureStats`, `UpdateMaterialTaskInventory`,
`UploadAgentProfileImage`, `ViewerBenefits`, `ViewerMetrics`, `ViewerStats`.

## Do first

The ones that change a path a viewer here already takes, roughly by how much a
resident sees:

- `AgentProfile` / `UploadAgentProfileImage` — the modern profile, which
  replaces the UDP `AvatarPropertiesRequest` family on Second Life.
- `SetDisplayName` — [[viewer-display-name-set]] needs it.
- `HomeLocation` — set home over HTTP (both grids).
- `ParcelPropertiesUpdate` — parcel edits over HTTP (both grids);
  [[viewer-parcel-config-missing-writes]].
- `DispatchRegionInfo`, `EstateAccess`, `ModifyRegion` — region and estate
  edits.
- `RequestTaskInventory`, `UpdateGestureTaskInventory`,
  `UpdateMaterialTaskInventory` — task inventory over HTTP
  ([[gridspec-task-inventory]]).
- `AcceptFriendship` / `DeclineFriendship` — the friendship answers.
- `UpdateAgentLanguage` — [[viewer-i18n-agent-language]].
- `AvatarRenderInfo` — [[protocol-avatar-render-info]].
- `GroupAPIv1` — [[viewer-social-group-extras]].
- `ViewerBenefits` — refreshing the benefits package mid-session.

## Done when

Each name is either requested and used with the old path kept for regions that
refuse it — with the fake grid serving it per flavour and the
`seed-capabilities` case's refused sets updated — or written down in the book
chapter as deliberately not adopted, and why.
