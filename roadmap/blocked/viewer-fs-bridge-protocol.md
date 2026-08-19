---
id: viewer-fs-bridge-protocol
title: Firestorm LSL bridge — the viewer↔script protocol and what it exposes
topic: viewer
status: blocked
origin: user request (2026-07)
blocked_by: [viewer-fs-bridge-lifecycle]
---

Context: [context/viewer.md](../context/viewer.md).

Once the bridge is worn ([[viewer-fs-bridge-lifecycle]]), the viewer and its
script talk over a **two-channel, asymmetric protocol** — and everything
Firestorm-specific that "the viewer somehow knows" rides on it. This task is
that protocol and the features it unlocks. Like the lifecycle it is a Firestorm
extension, not a Linden protocol, and stays **opt-in** (off by default).

The protocol (`fslslbridge.cpp`):

- **Handshake.** The script calls `llRequestURL` and `llOwnerSay`s a pseudo-XML
  line: `<bridgeURL>…</bridgeURL><bridgeAuth>…</bridgeAuth><bridgeVer>…
  </bridgeVer>`. The viewer parses it out of ordinary **owner-say chat**
  (`FSLSLBridge::lslToViewer`, hooked into the chat path next to the RLVa hook),
  checks `bridgeAuth` against the inventory object it actually created (a
  stranger's object must not impersonate the bridge), checks the version, stores
  the URL, and answers `URL Confirmed`. `<bridgeRequestError/>` means the region
  would not give the script a URL.
- **Viewer → script: HTTP POST** to that LSL URL
  (`FSLSLBridge::viewerToLSL`), body a plain **pipe-delimited** string:
  `getScriptInfo|<uuid>|<extended>`, `UseLSLFlightAssist|<n>`,
  `UseMoveLock|1|noreport`, `llMoveToTarget|<vec>|<host>`,
  `ExternalIntegration|<oc>|<lm>`, `Response_to_response|…`. So the *request*
  half is not chat at all — it is the region's `llRequestURL` HTTP endpoint,
  which is why the handshake has to happen first and why a URL-less region kills
  the feature.
- **Script → viewer: chat**, tagged lines the viewer sniffs and swallows:
  `<clientAO …>` (a script asking the viewer's built-in animation overrider to
  switch state), `<bridgeGetScriptInfo>…` (the answer to `getScriptInfo`),
  `<bridgeMovelock …>`, `<bridgeError …>`. Same trick as RLVa: an ordinary
  `llOwnerSay` the viewer intercepts and never shows the user.

What it buys:

- **Script info for an object** (`getScriptInfo`) — running scripts, memory,
  URLs used — which no UDP/CAPS message gives a client; the sim only tells a
  *script* (`llGetObjectDetails` on `OBJECT_RUNNING_SCRIPT_COUNT` etc.).
- **Movelock** and **flight assist** — the viewer asking the script to
  `llMoveToTarget` / apply impulses on the agent, movement effects only an
  in-world script may perform.
- **AO integration** and third-party hooks (OpenCollar / LockMeister
  `ExternalIntegration`), plus the avatar Z-offset / height plumbing Firestorm
  users know from the bridge.

Load-bearing to port faithfully: the security check on `bridgeAuth` — without it
any owned object could claim to be the bridge and be handed the viewer's trust.

Test end-to-end on OpenSim (it supports `llRequestURL` and the scripted-object
recipe already used for other live tests): wear the bridge, ask it for
`getScriptInfo` on a scripted prim, and assert the reply comes back through the
chat tag path.

Reference (Firestorm, read-only): `fslslbridge.cpp` / `fslslbridge.h`
(`lslToViewer`, `viewerToLSL`, the `bridgeAuth` check, the tag parsing).

## Parity-audit addendum (2026-08-19)

Additions from the audit of `fslslbridge.cpp` and its call sites:

- `getZOffsets|<uuid,csv>` (fsradar.cpp:659-680): batched — at most
  FSRADAR_MAX_OFFSET_REQUESTS UUIDs per POST — and the only command
  answered in the HTTP response body (a `uuid:z` CSV, only for
  avatars above z=1023) rather than over chat.
- `RelockMoveLockAfterMovement|<0/1>` (llviewercontrol.cpp:873): the
  script re-locks movement when controls are released.
- `DetachBridge`: the script supports it (llDetachFromAvatar) but the
  current Firestorm viewer never sends it.
- The `bridgeError` code set: `injection` (foreign-script injection
  detected), `scriptinfonotfound` (bad target), `wrongvm` (LSO VM
  warning).
- The `<clientAO state=...>` mapping: on/off toggle AO pause
  (PauseAO), standon/standoff toggle the stands setting (UseAOStands),
  each with a nearby-chat notice (FSAOScriptedNotification); the AO
  being controlled is [[viewer-animation-overrider]] (whose body is
  silent on scripted control — the handoff lands here).
- The 27-field extended `getScriptInfo` report (vs the 6-field basic
  one), selected by the `FSScriptInfoExtended` preference; several
  fields are base64-encoded, and it includes pathfinding time.
- Radar-row Z-offset integration: the radar itself is done
  ([[viewer-avatar-radar]]) and its record documents "no LSL-bridge
  altitude correction" as a pending divergence — since that task is
  closed, wiring `getZOffsets` results into radar rows belongs here.
