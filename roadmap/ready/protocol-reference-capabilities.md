---
id: protocol-reference-capabilities
title: Request and use the capabilities the reference viewer asks for and the grids grant
topic: protocol
status: ready
origin: gridspec-seed-capabilities (2026-10-04)
refs: [gridspec-seed-capabilities, viewer-display-name-set, viewer-social-group-extras,
  viewer-i18n-agent-language, protocol-avatar-render-info, gridspec-task-inventory,
  viewer-parcel-config-missing-writes]
---

Context: [context/protocol.md](../context/protocol.md).

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
