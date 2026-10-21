---
id: gridspec-avatar-render-info
title: AvatarRenderInfo and AttachmentResources on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-avatar-render-info, viewer-script-limits,
  viewer-avatar-complexity-limit]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-21)

The `avatar-render-info` census (2av, live grids; five runs on aditi, three
on OpenSim, the last of each with every answer held) reads both
capabilities raw and through our client: what A is told of the scripts it
wears, a cube asking for a URL attached from the world and taken off again;
then, on Second Life, both avatars move to Ahern, where nobody else reports,
and B reports A, A reports itself, B reports A as too complex, and the
refusals (`book/src/gridspec/avatars.md` § Render info). Findings:

- Second Life grants `AvatarRenderInfo` only to a seed request that names
  it; ours did not, so the first aditi run found it "not granted". The client
  now requests it (`CAP_AVATAR_RENDER_INFO`, a `Pending` row of the server
  coverage table); OpenSim refuses it (`OPENSIM_REFUSED_CAPABILITIES`).
- A lone report is stated as sent within 0.2 s, to the subject and the
  reporter alike. More reports give a weight that is neither report nor their
  mean, and the region later states weights of its own (B appears, reported
  by nobody of ours, near its real complexity). `reportinglimit` counts
  reports of the reader (B's two count twice); `overlimit` did not move for a
  `tooComplex` report in a quiet region. Every POST, malformed or of nobody,
  is `200 {}`; `PUT` / `DELETE` are `405`.
- In the sandbox, other residents' viewers report on our avatars: a census of
  this capability has to run where nobody else is.
- `AttachmentResources` is the same document on both grids, by LSL point
  names (`ATTACH_RIGHT_PEC`), available memory -1 and 38 URLs; OpenSim labels
  it `text/html;charset=UTF-8` and never states `memory`; Second Life states
  65,536 for a Mono script. A URL held by a prim survives its attach from the
  world on both.

Fake grid: each flavour now answers `AttachmentResources` with the measured
nothing-scripted report (it used to send an empty summary), labelled as its
grid does (`ImitatedGrid::attachment_resources_label`, held by a `sl-proto`
test). `AvatarRenderInfo` stays with `protocol-avatar-render-info`, now
unblocked. The census is not run offline: the fake grid wears no attachments
and has no render info.

Viewer: our client reads the attachment report under either label. It shows
no region-stated weight and no jellydoll notice (`protocol-avatar-render-info`)
and has no floater for the attachment report (`viewer-script-limits`).

`protocol-avatar-render-info` followed in the same commit. It adds the
client's reads and reports, the Second Life flavour's capability, the offline
census and the viewer's Reported column, name-tag figure and jellydoll notice.

## Known already

OpenSim registers `AttachmentResources`, has no `AvatarRenderInfo`; no client or
server code for the latter.

## Discover

`sl-repl --script` cap probes on both grids; Firestorm's
`llavatarrenderinfoaccountant.cpp`; a second avatar for others' reports.

## Document

`book/src/gridspec/avatars.md` § Render info.

## Fake grid

Small once the protocol exists — [[protocol-avatar-render-info]].

## Viewer

Report own complexity and show others' on SL; nothing on OpenSim.
