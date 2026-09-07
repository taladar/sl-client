---
id: viewer-rlva-floaters-toggles
title: "RLVa UI: console, restrictions/strings/locks floaters + toggles"
topic: viewer
status: in-progress
origin: main-menu survey (2026-07-23)
blocked_by: [viewer-rlv-restriction-state]
refs: [viewer-rlv-command-parser, viewer-rlv-notify, viewer-rlv-queries]
---

Context: [context/viewer.md](../context/viewer.md).

The `viewer-rlv-*` tasks build the RLV *engine* (parser, enforcement,
queries, state). This task is the user-facing RLVa control surface —
Firestorm's whole RLVa top-level menu:

- **Console…** (`rlv_console`): type RLV commands at the viewer, see
  responses — the debugging/authoring tool.
- **Restrictions…** (`rlv_behaviours`): live list of active restrictions
  grouped by source object, reading the restriction registry
  ([[viewer-rlv-restriction-state]]).
- **Strings…** (`rlv_strings`): the editable response-strings table
  (customise the canned texts RLVa emits).
- **Locks…** (`rlv_locks`): inspector for attachment/wearable locks.
- Behaviour toggles: Allow OOC Chat, Show Filtered Chat, Show Redirected
  Chat Typing, Split Long Redirected Chat, Allow Temporary Attachments,
  Forbid Give to #RLV, Wear Replaces Unlocked, and the Debug submenu.

Scope: the four floaters, the RLVa top-level menu with its toggles
(settings-backed, consumed by the enforcement layers), all gated on RLV
being enabled.

Reference (Firestorm, read-only): `menu_viewer.xml` RLVa menu
(~L2835-3053), `rlvfloater*` sources, `RestrainedLove*`/`RLVa*`
settings.

Builds on: the restriction-state registry (its data model powers the
Restrictions floater); the toggles thread into the enforcement tasks.

## Parity-audit addendum (2026-08-19)

Minor addition from the audit: the settings backing is the full
RlvSettingNames roster (rlvdefines.h:402+) — RestrainedLove (master),
RestrainedLoveDebug, CanOOC, ShowEllipsis, ForbidGiveToRlv, NoSetEnv,
WearAddPrefix / WearReplacePrefix, RLVaEnableIMQuery,
EnableLegacyNaming, EnableSharedWear, EnableTempAttach,
HideLockedLayers / HideLockedAttachments / HideLockedInventory,
LoginLastLocation (RLVaLoginLastLocation gates login-to-last-location),
SharedInvAutoRename, ShowAssertionFailures, ShowRedirectChatTyping,
SplitRedirectChat, TopLevelMenu, WearReplaceUnlocked. Also note
RestrainedLoveDebug echoes every processed command into the console.

## Done

Two new places. `sl-viewer-world-api::rlv` holds the viewer's **one**
`RlvSession` — the `RlvState` machine, a restriction revision a floater
rebuilds on, and the console transcript — plus the whole `RlvSettingNames`
roster and the eight customisable response strings. It is in the world-API
tier because the floaters only *draw* this state while the chat bar, the
session command path and every wear path have to *ask* it, and none of those
may depend on a UI crate.

`sl-viewer-rlv` is the four windows: `rlv_console`, `rlv_behaviours`,
`rlv_locks`, `rlv_strings`. The RLVa top-level menu, its condition wiring and
its dispatch live in `menu_bar.rs`, between Content and Help.

**The toggles are one table, not twenty.** `RLV_BOOL_SETTINGS` carries each
flag's name, default, description *and* menu-condition key, so the settings
registrar, the check marks and the dispatch all read the same row. A menu
toggle's **action string is its setting name**, which is what lets the
dispatch be one roster lookup (`is_rlv_setting`) rather than twenty arms that
could each name a setting the roster does not have.

**Deliberate divergences from the reference**, each with its reason:

- **The RLVa menu is always in the bar; its entries are greyed while RLV is
  off** (the master switch stays live, since it is what turns it on). The
  reference leaves every line pickable and lets the *commands* fail instead. A
  greyed line says so before the click.
- **`RLVaTopLevelMenu` is not registered.** It chooses between a top-level
  RLVa menu and a copy embedded under Advanced; there is one placement here,
  so it would be a setting nothing reads.
- **The Strings floater's edits take effect at once.** The reference writes to
  its own `rlv_strings.xml`, reads it only at startup, and so has to ask for a
  relog. As ordinary settings there is nothing to relog for.
- **Only the eight `customizable` strings are listed** — as in the reference.
  The `hidden_*` / `blocked_*` texts are viewer voice and belong to the
  translated notification catalogue.
- **The console has no `RET:` stream.** `RlvOutcome` has no retained variant —
  a command is applied or refused there and then — so nothing could fill it.

## Not done — and why

- **`@get*` queries typed at the console are accepted but not answered.**
  `RlvState::answer` needs an `RlvQuerySource`, and a faithful one needs facts
  this viewer does not have yet: the shared `#RLV` folder tree (nothing builds
  it — [[viewer-inventory-folder-tree]]) and the agent's hover height (its own
  task, [[viewer-agent-hover-height-ingest]]). A half-wired source would
  answer `@getattach` with all-zeros for a fully dressed avatar, which is
  worse than not answering, so the console says so on the line instead. The
  rest of the source (attachments from `ObjectState`, wearables from the COF,
  camera, group, sit target) is available and is the integration
  [[viewer-rlv-queries]] should do.
- **`RestrainedLoveDebug` is registered but nothing yet fills the console from
  it.** It echoes *processed* commands, and today only the console processes
  commands — and it echoes regardless. It becomes load-bearing with the
  owner-say command intake, which no task owns yet and which is the obvious
  next RLV step.
- **The Locks window does not list the `nostrip` soft locks**, nor resolve a
  folder lock to the worn items it catches. Both need the `#RLV` folder tree.
  The four lock registries themselves are complete.
- **The issuer column shows the object's key**, with its attachment point
  where the state machine has been told one. Resolving a key to an item name
  needs a lookup the viewer lacks (`ObjectProperties` are kept only for the
  selection); the reference prints the key too whenever it cannot do better.

## Verified

`cargo test --release -p sl-viewer-rlv -p sl-viewer-world-api` — 49 tests
green. `cargo test --release -p sl-client-bevy-viewer` — 294 + 10 green,
including the re-pinned menu-bar action table, the four `FLOATERS` registry
guards and the blessed settings golden. `cargo clippy --release` clean across
all three crates.

Not verified live: the four windows' content and the menu's greying were not
driven against a grid.
