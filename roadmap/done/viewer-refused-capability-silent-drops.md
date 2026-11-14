---
id: viewer-refused-capability-silent-drops
title: Commands for a capability the grid refuses are dropped without a word
topic: viewer
status: done
origin: gridspec-seed-capabilities audit (2026-10-04)
refs: [gridspec-seed-capabilities]
---

Context: [context/viewer.md](../context/viewer.md).

The `gridspec-seed-capabilities` audit checked, for every capability a stock
OpenSim refuses (`book/src/gridspec/capabilities.md`), what both runtimes do
when a command needs it. Three gaps with a real fallback were fixed then (user
reports, offline messages, material edits). What remains are commands with
**no** fallback on such a grid that are dropped silently — `if let Some(url) =
caps.get(..) { … }` with no `else` — so a floater waits for a reply that never
comes:

- **All experience commands** (`GetExperienceInfo`, `FindExperienceByName`,
  `GetExperiences`, `ExperiencePreferences`, `AgentExperiences`,
  `GetAdminExperiences`, `GetCreatorExperiences`, `GroupExperiences`,
  `IsExperienceAdmin`, `IsExperienceContributor`, `UpdateExperience`,
  `RegionExperiences`, `ExperienceQuery`) — tokio `lib.rs` around 2354–2437,
  bevy `lib.rs` around 3930–4099. The experience floaters get nothing on
  OpenSim.
- **Voice** (`RequestVoiceAccount`, `RequestParcelVoiceInfo`,
  `SendVoiceSignaling`) — tokio `lib.rs` around 2280, bevy around 3785.
- **The explicit `Ais3*` commands** — only reachable from `sl-repl`.
- **bevy only:** before the region's capabilities arrive, the upload-failure
  and marketplace-failure reports are skipped (`upload.rs` around 74,
  `lib.rs` around 4595), where tokio reports them.

Fix: report each as a failure the caller can see (the
`report_caps_failure` path the material edit now takes), and have the viewer
say "not available on this grid" where it would otherwise wait — or disable
the floater up front from the granted capability set, as the reference
viewer disables its material editor.

## Resolution

Both runtimes now report every one of these commands they cannot send:
`report_capability_not_granted` (in each driver's `lib.rs`) logs it and, with
diagnostics on, emits `Diagnostic::ExpectedReplyMissing` naming the
capability — for the thirteen experience commands, the three voice requests and
the eight explicit `Ais3*` commands. It reports straight from the command loop,
not through the caps channel's failure sentinel: in `sl-client-tokio` that
channel is bounded and drained by the same loop, so the material edit's
`report_caps_failure` from inside the loop could wait on itself (it and the
two `ModifyRegion` reports now use the helper too); in `sl-client-bevy` there
is no channel before the region's capabilities arrive, which is also why a
material edit then only logged. `sl-client-bevy` now reports an upload
(`AssetUploadFailed`) and a marketplace request (its failure event) made
before the capabilities arrive, straight to the app side, where it skipped
both.

The viewer asks before it would wait. `sl-client-bevy` keeps the names of the
last capability map as the `SlGrantedCapabilities` resource (`grants` /
`refuses`, neither before a map arrives). The Experiences floater and an
experience profile raise the new `ViewerExperiencesUnavailable` notice instead
of asking a region that grants no `GetExperiences` / `GetExperienceInfo`; an
experience search (the floater's Search tab and the picker) shows "Experiences
are not available on this grid." instead of "Searching…"
(`SearchProgress::Unavailable`, `SearchProgress::start`); Region / Estate's
Experiences tab shows a note in place of its three lists and asks for them
only once the region is known to grant `RegionExperiences` — the reference
removes that tab. The automatic experience requests (the environment's
`ExperienceQuery`, the script editor's and the permission card's
`GetExperienceInfo`) only follow data a grid with experiences sends, and are
left as they are; the driver reports them if they ever go out regardless.

Unit tests pin the bevy driver's reports (missing capability with and without
diagnostics, upload and marketplace before the capabilities), the grants
resource, the floater's requests on a granting, refusing and unknown region,
the search's start, and the Region / Estate tab waiting for and lacking the
capability. `client_end_to_end`'s
`commands_without_their_capability_are_reported` sends experience, voice and
AIS3 commands to the OpenSim-flavoured fake grid through `sl-client-tokio` and
reads each report back. The e2e stage
`experiences_say_they_are_unavailable_on_the_opensim_flavour` (`e2e_windows`)
checks the note, the notice and the search status on the OpenSim flavour; the
Second Life flavour's existing experience stages cover the granted side.
