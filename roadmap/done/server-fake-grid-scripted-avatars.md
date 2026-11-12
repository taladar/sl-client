---
id: server-fake-grid-scripted-avatars
title: Scripted avatars — a client that grants, pays, sits and answers
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20); raised by the user while
  scoping the scripts scenario
points: 8
blocked_by: [server-fake-grid-script-engine-wiring]
refs: [test-e2e-stage, server-fake-grid-scripted-scenario, server-lsl-lib-money-permissions,
  server-fake-grid-prim-sit-states-no-sit-animation, server-fake-grid-scripted-avatar-clothes-not-shown,
  server-lsl-lib-avatar-control, test-fake-grid-npc-avatars]
---

Context: [context/lsl.md](../context/lsl.md).

Most of what a script does to an avatar cannot be tested without an
avatar that answers back. `llRequestPermissions` raises a question and
waits for a `ScriptAnswerYes`; `llDialog` waits for a button on a hidden
channel; `money` fires only when somebody actually pays; a pose ball's
`llAvatarOnSitTarget` is `NULL_KEY` until somebody sits; and
`llStartAnimation` has nothing to animate. None of that can come from an
NPC: the fake grid's `NpcFixture`s ([[test-fake-grid-npc-avatars]]) are
avatar-shaped *objects* — a body, an `AvatarAppearance`, an
`AvatarAnimation`, attachments — with no circuit, no session and nothing
that can be asked a question.

So a scenario that exercises the avatar-facing half of the library needs
a **second real client**, and the scenario should be able to describe
what it does rather than a human being the only one who can play it.

Wanted:

- a **scripted avatar**: a headless `sl-client-tokio` session the
  scenario starts, logging in as one of the grid's other accounts (the
  crate already supports several, and `sl-conformance` already runs
  `2av` / `3av` cases with `--secondary` / `--tertiary`), driven by a
  small script of its own — walk here, touch that, answer the next
  dialog with button *n*, grant these permissions, pay this object L$50,
  sit on that prim, stand, say this on channel 0, wait for that;
- expressed in the **existing timeline vocabulary** where it fits.
  `crate::timeline` is already "a scripted sequence of grid-side actions
  for one avatar" with `At::OnEvent` predicates and a hand-over on
  teleport; a scripted avatar is largely the same idea driven from the
  *client* side instead of the grid side, and the two should share their
  step and predicate types rather than growing a second dialect;
- **answer policies** as the cheap default: "grant every permission this
  scenario is asked for", "answer every dialog with its first button",
  "pay any vendor that asks" — so a scenario that merely needs the grant
  to exist does not have to spell out a conversation. The bare policies
  are what unattended cross-check and conformance runs use; the explicit
  scripts are for cases that assert *which* answer was given;
- **a real appearance**, so an animated or seated scripted avatar is
  worth looking at in a frame — the grid already bakes and serves NPC
  appearances, and a scripted avatar should be able to wear one;
- and the discipline the rest of the crate has: deterministic (its
  actions are tied to ticks and events, never to sleeps), and it shuts
  down cleanly with the grid rather than leaking a session.

Note what this is *not*: a load generator and not an AI. It is the
second half of a two-party protocol test, and its value is that a
scenario can state both halves in one place.

Acceptance: the `scripts` scenario's pose ball, vendor and permission
prim are all exercised end to end in a `cargo test` with no human; a
case can assert that a specific button was answered and that the script
saw it; the scripted avatar appears in a `sl-crosscheck` frame seated
and animated; and two runs of one seed produce the same transcript.

## Progress (2026-11-12)

Implemented in `sl-fake-grid/src/scripted_avatars.rs`: a `ScriptedAvatar`
stated on a `RegionConfig` (`scripted_avatars`), whose account the builder
registers and which the grid logs in itself as an `sl-client-tokio` client
(now a normal dependency of `sl-fake-grid`). Its steps reuse the timeline's
`At` and the timeline's own wait code, against the avatar's own session;
`At::AfterTicks` is new and shared with timelines. Actions: walk, touch, sit,
stand, say, pay, answer a dialog (index, label or text), answer a permission
question, await a chat line. `AnswerPolicy` (grant/refuse permissions, first
button, pay a pay-click vendor its default price) answers what no step does.
Every prompt and answer goes into a transcript of names, labels and amounts.
`FakeGrid::scripted_avatars()` hands back handles (progress, transcript,
log-out); a grid shutdown stops them.

`fixtures::scripted_avatar` is the scene: a pose ball (carrying its own
animation), a vendor, a permission prim and a dialog menu, and the named
scenario `scripted-avatar` adds `sitter()`. `tests/scripted_avatars.rs`
holds the whole conversation end to end on both flavours, a specific dialog
button answered and heard by its script, an ignored question and a refusal,
two seeded runs giving one transcript, and a late viewer told the sitter's
pose. Fixed on the way: a script's animation of an avatar
(`scripts.rs` `state_animations`) was stated only to that avatar's own
viewer, never to the region's others.

Differences from the acceptance as written:

- The `scripts` scenario does not exist yet
  (`server-fake-grid-scripted-scenario` is blocked), so the pose ball, vendor
  and permission prim are this task's own `scripted-avatar` scene.
- The `sl-crosscheck` frame (sl-client side only) shows the avatar on the
  pose ball and animated (its torso twists between frames), but standing
  upright: the fake grid states no sit animation
  (`server-fake-grid-prim-sit-states-no-sit-animation`). The pose is a
  fixture animation, not `dance1`, because the fake grid serves no Linden
  animation assets for a viewer to fetch.
- The avatar's account is `dressed`, but its clothes do not show in the
  frame (`server-fake-grid-scripted-avatar-clothes-not-shown`).
