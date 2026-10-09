# Capabilities

Which capabilities each grid's seed grants. A region's seed answers a list of
names with the subset it grants, and that subset decides which protocol path a
viewer takes for almost everything: an HTTP capability where the grid offers
one, a UDP message or nothing where it does not. How the seed works is in
[Capabilities](../comms/caps.md).

Measured on 2026-10-04 by the conformance case `seed-capabilities`: one login
with the client's own list (`REQUESTED_CAPABILITIES`, then 67 names), then a
relogin asking for that list together with the reference viewer's
(Firestorm's `LLViewerRegionImpl::buildCapabilityNames`, 116 names). The case
holds each live grid to exactly the refused set below, and each fake flavour
to refusing at least that set. Each capability adopted since is granted by
both grids and re-measured on adoption: with `ParcelPropertiesUpdate`
(2026-10-05) the list is 68 names, aditi grants 66 and OpenSim 35, and the
refused sets are unchanged. `InterestList` (2026-10-09) is the first adopted
capability OpenSim does not grant: the list is 69 names, aditi grants 67, and
OpenSim's refused set is one longer.

## The client's own list

| | Second Life (aditi) | OpenSim (local, stock) | fake grid |
| --- | --- | --- | --- |
| granted | 65 of 67 | 34 of 67 | `FakeSl` 60, `FakeOpensim` 33 |
| refused | `ObjectAnimation`, `UploadBakedTexture` | 33: see below | each flavour refuses its grid's set (`ImitatedGrid::withheld_capabilities`), plus what it does not implement |

What OpenSim refuses, grouped by what a viewer loses:

| area | refused on OpenSim | what a viewer does instead |
| --- | --- | --- |
| inventory | `InventoryAPIv3`, `LibraryAPIv3`, `FetchLib2`, `FetchLibDescendents2` | `FetchInventoryDescendents2` / `FetchInventory2` for the agent's tree and its background crawl, UDP `FetchInventoryDescendents` for the Library |
| experiences | `AgentExperiences`, `ExperiencePreferences`, `ExperienceQuery`, `FindExperienceByName`, `GetAdminExperiences`, `GetCreatorExperiences`, `GetExperienceInfo`, `GetExperiences`, `GroupExperiences`, `IsExperienceAdmin`, `IsExperienceContributor`, `RegionExperiences`, `UpdateExperience` | nothing: stock OpenSim has no experiences (the client drops the requests without a word — `viewer-refused-capability-silent-drops`) |
| voice | `ProvisionVoiceAccountRequest`, `ParcelVoiceInfoRequest`, `VoiceSignalingRequest` | nothing: a stock region loads no voice module (dropped silently, as above) |
| groups and chat | `AcceptGroupInvite`, `DeclineGroupInvite`, `ChatSessionRequest` | the UDP `ImprovedInstantMessage` forms; a session's server-side history is not available |
| messages | `ReadOfflineMsgs` | the UDP `RetrieveInstantMessages` (the client falls back itself since 2026-10-04) |
| account | `UserInfo` | the UDP `UserInfoRequest` / `UpdateUserInfo` |
| appearance | `UpdateAvatarAppearance`, `IncrementCOFVersion` | the viewer bakes and uploads (`UploadBakedTexture`, which OpenSim grants) |
| materials | `ModifyMaterialParams`, `UpdateMaterialAgentInventory` | no PBR overrides or material saving; an edit is reported as failed rather than dropped (since 2026-10-04) |
| objects | `InterestList` | nothing: OpenSim sends a region's objects whichever way the camera looks (since 2026-10-09) |
| other | `DirectDelivery`, `SendUserReport`, `SendUserReportWithScreenshot`, `ObjectAnimation` | marketplace delivery reports its absence; a report goes over the UDP `UserReport`, without a screenshot (since 2026-10-04); `ObjectAnimation` is only named to opt in to the UDP stream |

How each runtime handles each refused capability was audited on 2026-10-04,
both runtimes alike. The three gaps it found — the report and offline-message
fallbacks and the dropped material edit — are fixed and tested against the
OpenSim-flavoured fake grid (`client_end_to_end`) or live OpenSim
(`offline-msg-fetch`).

`ObjectAnimation` is refused by both grids: it is a name our list carries that
neither grid offers.

## What only the reference viewer asks for

Of the 51 names Firestorm requested that our list did not on 2026-10-04,
**aditi grants 48** — among them `AgentProfile`, `SetDisplayName`,
`HomeLocation`, `ParcelPropertiesUpdate`, `DispatchRegionInfo`, `EstateAccess`,
`EnvironmentSettings`, `RequestTaskInventory`, `ViewerBenefits`,
`InterestList`, `GroupAPIv1`, `UpdateAgentInformation`, `UpdateAgentLanguage`,
`UpdateGestureTaskInventory`, `UpdateMaterialTaskInventory`,
`UploadAgentProfileImage`, `ViewerStats`, `AcceptFriendship` and
`DeclineFriendship` — and refuses `DispatchOpenRegionSettings`,
`EstateChangeInfo` and `ViewerStartAuction`. **OpenSim grants 12** of them:
`EnvironmentSettings`, `EstateAccess`, `HomeLocation`, `MapLayer`,
`MeshUploadFlag`, `ParcelPropertiesUpdate`, `ServerReleaseNotes`,
`SimConsoleAsync`, `UntrustedSimulatorMessage`, `UpdateAgentInformation`,
`UpdateAgentLanguage` and `UpdateGestureTaskInventory`.

What each of them does, its request and response shapes, whether it is current
or deprecated, and which roadmap task adopts it are in the
[Capability Reference](../comms/caps-reference.md). Neither grid granted a name
nobody asked for.

## Neighbour regions

| | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a child region's grant | not measured: the test avatar's start region (Mauve) has no neighbours | identical to the root region's, on all three neighbours | identical to the root's |
