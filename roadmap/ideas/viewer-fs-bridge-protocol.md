---
id: viewer-fs-bridge-protocol
title: Firestorm LSL bridge — the viewer↔script protocol and what it exposes
topic: viewer
status: ideas
origin: user request (2026-07)
refs: [viewer-fs-bridge-lifecycle]
blocked_by: [viewer-fs-bridge-lifecycle]
---

Context: [context/viewer.md](../context/viewer.md).

Once the bridge is worn ([[viewer-fs-bridge-lifecycle]]), the viewer and its
script talk over a **two-channel, asymmetric protocol** — and everything
Firestorm-specific that "the viewer somehow knows" rides on it. This task is
that protocol and the features it unlocks.

The protocol (`fslslbridge.cpp`):

- **Handshake.** The script calls `llRequestURL` and `llOwnerSay`s a pseudo-XML
  line:
  `<bridgeURL>…</bridgeURL><bridgeAuth>…</bridgeAuth><bridgeVer>…</bridgeVer>`.
  The viewer parses it out of ordinary **owner-say chat**
  (`FSLSLBridge::lslToViewer`, hooked into the chat path next to the RLVa hook),
  checks `bridgeAuth` against the inventory object it actually created (a
  stranger's object must not impersonate the bridge), checks the version, stores
  the URL, and answers `URL Confirmed`. `<bridgeRequestError/>` means the region
  would not give the script a URL.
- **Viewer → script: HTTP POST** to that LSL URL (`FSLSLBridge::viewerToLSL`),
  body a plain **pipe-delimited** string: `getScriptInfo|<uuid>|<extended>`,
  `UseLSLFlightAssist|<n>`, `UseMoveLock|1|noreport`,
  `llMoveToTarget|<vec>|<host>`, `ExternalIntegration|<oc>|<lm>`,
  `Response_to_response|…`. So the *request* half is not chat at all — it is the
  region's `llRequestURL` HTTP endpoint, which is why the handshake has to
  happen first and why a URL-less region kills the feature.
- **Script → viewer: chat**, tagged lines the viewer sniffs and swallows:
  `<clientAO …>` (a script asking the viewer's built-in animation overrider to
  switch state), `<bridgeGetScriptInfo>…` (the answer to `getScriptInfo`),
  `<bridgeMovelock …>`, `<bridgeError …>`. Same trick as RLVa: an ordinary
  `llOwnerSay` the viewer intercepts and never shows the user.

What it buys, and the honest reason to want it:

- **Script info for an object** (`getScriptInfo`) — running scripts, memory,
  URLs used — which no UDP/CAPS message gives a client; the sim only tells a
  *script* (`llGetObjectDetails` on `OBJECT_RUNNING_SCRIPT_COUNT` etc.).
- **Movelock** and **flight assist** — the viewer asking the script to
  `llMoveToTarget` / apply impulses on the agent, i.e. movement effects only an
  in-world script may perform.
- **AO integration** and third-party hooks (OpenCollar / LockMeister
  `ExternalIntegration`), plus the avatar Z-offset / height plumbing Firestorm
  users know from the bridge.

Two things to settle before building it: (1) it is a **Firestorm extension**,
not a Linden protocol — implementing it makes `sl-client` interoperate with
Firestorm's ecosystem, and it should stay clearly optional (a setting, off by
default, since creating and wearing an object on the user's behalf is a side
effect a library must not perform unasked); (2) the security check on
`bridgeAuth` is load-bearing — without it any owned object could claim to be the
bridge and be handed the viewer's trust, so port it faithfully.

Test end-to-end on OpenSim (it supports `llRequestURL` and the scripted-object
recipe already used for other live tests): wear the bridge, ask it for
`getScriptInfo` on a scripted prim, and assert the reply comes back through the
chat tag path.
