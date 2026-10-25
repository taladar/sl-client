---
id: gridspec-lsl-experience
title: LSL library behaviour on each grid — experiences and the key-value store
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-experience, gridspec-lsl-in-experience]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: Experience permissions and key-value store (aditi only; the probe
needs an experience). Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Experiences and the key-value store.

## Fake grid

Large — [[server-lsl-lib-experience]].

## Viewer

Experience permission card.

## Done (2026-10-25)

Measured **outside every experience** only, as agreed: none of our avatars
owns, administers or contributes to an experience, so none can save a
script into one (creating one needs Premium). What a script *in* an
experience sees is [[gridspec-lsl-in-experience]].

- **Discover.** Two probes, `sl-lsl-runtime/probes/experience/`:
  `no-experience` (`llAgentInExperience`, `llGetExperienceDetails` of the
  script's own, a public stranger's and an unknown id, every
  `llGetExperienceErrorMessage` code, `llRequestExperiencePermissions` of
  the owner and the other avatar, the seven key-value calls and their
  `dataserver` answers) and `powers` (`llTeleportAgent` of the owner and of
  the other avatar, with and without `PERMISSION_TELEPORT`,
  `llTeleportAgentGlobalCoords`, `llAttachToAvatarTemp`). Complete on aditi
  and both OpenSim engines. Second Life denies at once with reason 5 and no
  question, answers every key-value call `0,5` out of call order, returns
  `llGetExperienceDetails` in an order the library table misstates, and
  outside an experience teleports the owner only — through Mauve's telehub,
  not to the aim. OpenSim compiles none of the experience functions (YEngine
  lists each; XEngine stops at the handler), and teleports a granting
  non-owner as well as the owner, to the aim.
- **Runner.** Notes `# teleport-started`, `# teleport-local` and
  `# teleport-failed` for either avatar, and walks an avatar a script
  teleported inside the region back to the build location before the next
  probe.
- **Not measured.** `llSitOnLink`, `llClearExperiencePermissions` and
  `llGetExperienceList`: the vendored library table lacks them, so no probe
  using them compiles here ([[server-lsl-library-table-refresh]]). A
  temporary attachment to the other avatar would hand it the prim, which the
  runner then could not clear away.
- **Document.** `book/src/gridspec/lsl.md` § Experiences and the key-value
  store, linked from `content/experiences.md` and the experiences chapter's
  unmeasured list.
- **Fake grid.** Large, as planned: the rows are in
  [[server-lsl-lib-experience]]. No flavour knob: the fake grid runs no
  scripts.
- **Viewer.** Nothing of it reaches the viewer from a script in no
  experience. `e2e_script_permissions` now also holds the experience card
  on the fake Second Life flavour to a question under an experience, as the
  reference reads it: *Yes* grants the mask and admits the experience.
