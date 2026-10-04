---
id: gridspec-profiles
title: Profiles, picks, classifieds, notes and display names on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-display-name-set,
  viewer-mute-entry-name-is-not-the-username]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim ignores the picks / classifieds / notes `GenericMessage` queries,
volunteers replies after edits, cannot read notes back, breaks classified
delete; display names: SL-only pushes and set cap. None of the profile cases
ran on aditi.

## Discover

The profile cases on aditi; display-name reads (set only with the user's consent
— rate-limited for days); profile web URL caps.

## Document

`book/src/gridspec/profiles.md`.

## Fake grid

Display names small in this task; profiles large —
[[protocol-sim-profile-messages]] then [[server-fake-grid-profiles]].

## Viewer

Picks from volunteered replies on OpenSim; blank notes; display-name UI hidden
without the cap.

## Capabilities done in this task

[[protocol-cap-agent-profile]]: profiles over `AgentProfile`. Also the
protocol halves of `UploadAgentProfileImage`
([[viewer-profile-image-editing]]), `SetDisplayName`
([[viewer-display-name-set]]) and `SearchStatRequest`
([[viewer-classified-click-stats]]): the command, the reply, the fake grid's
Second Life flavour serving it; the UI stays in those tasks.
