# Context — CHAT_ROADMAP.md

Non-task preamble from `CHAT_ROADMAP.md` (scope, protocol/implementation
reality, locked decisions, and Phase-B consolidation notes). Tasks split out of
that file carry the `chat` topic; each Phase-A design item folds in its own
`reference (from A*)` section.

A plan to give the SL client a *stateful* chat-session system covering the three
instant-message session kinds — 1:1 direct IM, ad-hoc conferences, and group
chat — each potentially carrying **both a text and a voice channel** (voice at
the SL *signalling* level only — A12), with **friend presence** folded in. Today
this whole surface is a
stateless pass-through: inbound `ImprovedInstantMessage` is decoded and fanned
out to events (`InstantMessageReceived`, `ImTyping`,
`Group`/`ConferenceSessionMessage`, `…Participant`, `ConferenceInvited`), the
buddy list arrives once as `Event::FriendList`, and `OnlineNotification` /
`OfflineNotification` arrive as `Event::FriendsOnline` / `FriendsOffline` — but
**no `Session` state** tracks open sessions, rosters, typing, history, pending
invitations, or who is online. This roadmap plans a system that keeps that
state for the library user and resets the chat state tied to a friend when that
friend goes offline. Work these top-to-bottom; tick a box only when the step
builds, is clippy-clean (restriction lints), and `cargo test` passes. Add
sub-tasks as you discover them.

Phase A is **planning only** — its items produce design decisions, not code.
Phases B+ (implementation) are defined once Phase A is signed off.

Scope reminders:

- Commit on the current branch only (never auto-create a feature branch).
- `Session` (sl-proto) is sans-IO: the chat/presence state lives there, beside
  `TeleportPhase` / `SitState`, driven by inbound messages and the outbound
  commands.
- Keep `sl-client-tokio` and `sl-client-bevy` (and the REPL) at feature parity.
- Never push client-only protocol types into the shared `sl-types` crate.
- Local proximity chat (`ChatFromViewer` / `say` → `Event::ChatReceived`) is a
  **separate** concern, **out of scope** for the session-**state** model here
  (this roadmap is about IM / conference / group **sessions**) — but it **is**
  included in the optional **chat-log files** (A13), which cover *all* text-chat
  types (nearby + IM + group + conference).
- Optional **local chat-log files** (write + read-back, Firestorm-style; A13)
  in scope for **long-term** history beyond the in-memory cap; this is a
  **runtime** file-I/O feature (the sans-IO `Session` does no I/O), default off.
- A session's **voice channel is in scope at SL-signalling level** (has-voice,
  channel info, join/leave-voice, voice membership — A5 / A12), reusing the
  existing voice-signalling feature. The **Vivox/WebRTC audio transport and the
  "who is speaking" / talk-activity indicators are OUT of scope** (the external
  voice client's job); sl-client models voice *state*, not voice *audio*.
- Wrap this file at 80 columns; fmt/clippy/rumdl green before commit (the ggh
  hook rejects MD013 and re-runs clippy).

## Protocol reality (constraints Phase A must respect)

- One wire message carries all three chat kinds: `ImprovedInstantMessage`
  (`message_template.msg`, `Low 254`); the `ImDialog` byte (`types/chat.rs`)
  distinguishes the semantics and the `from_group` flag separates group
  (`true`) from conference (`false`) on `SessionSend` / `SessionAdd` /
  `SessionLeave`.
- Session-id semantics differ per kind: **1:1** = the deterministic
  `XOR(agent_id, peer)` (`compute_im_session_id` in `session/conversions.rs`);
  **conference** = a caller-minted `ImSessionId`; **group** = the group id
  itself.
- Modern invitations arrive over CAPS as `ChatterBoxInvitation` →
  `Event::ConferenceInvited`. The modern `ChatSessionRequest` capability
  (accept/decline and other session operations) is **not** implemented — only
  the UDP `ImprovedInstantMessage` path is. There is no accept/decline today;
  you join a session implicitly by sending into it.
- Inbound offline IMs already surface (`offline = true`), and **offline-IM
  history retrieval is now implemented** (A1 correction): the modern
  `ReadOfflineMsgs` CAPS (`Command::RequestOfflineMessages`,
  `offline_messages_from_llsd` in `session/conversions.rs`) *and* the legacy
  `RetrieveInstantMessages` UDP (`Command::RetrieveInstantMessages`,
  `send_retrieve_instant_messages` in `session/circuit.rs`) both ship — both
  re-deliver as offline `Event::InstantMessageReceived`. They were added by the
  `MISSING_ROADMAP.md` outbound work *after* this roadmap was drafted, so A8
  plans only the bounded per-session **log / unread** model, not the fetch path.
- Friend presence is **friends-only**, `CAN_SEE_ONLINE`-gated and bidirectional
  (confirmed in OpenSim `FriendsModule.cs`), and **passive** — the simulator
  pushes `OnlineNotification` / `OfflineNotification`; there is no
  `RequestOnlineNotification`. The rights flags are
  `sl_types::friend::FriendRights`: `CAN_SEE_ONLINE`, `CAN_SEE_ON_MAP`,
  `CAN_MODIFY_OBJECTS`.
- Chat sessions, history, and presence are **grid-level** (routed by the grid's
  IM / group / presence services, not the region simulator), so unlike
  `SitState` and script permissions they **persist** across teleport and region
  crossings — the *inverse* of those resets.
- No chat or presence state exists in the `Session` struct (`session.rs`); it
  would live beside the `TeleportPhase` / `SitState` enums (the precedent from
  commit `7bc19b4`).

## Other notes

## Phase B — design references (from Phase A)

Each Phase A item left a **reference** subsection here recording the design
knowledge it produced. The concrete implementation tasks these imply were
consolidated into **§ Phase B tasks — consolidated (B1–B10)** at the end of this
file (the `PERMISSION_ROADMAP.md` precedent). These reference subsections are
unchanged design records; only their bracketed task tags were re-pointed to the
new B-numbering.

> **Phase A SIGNED OFF — 2026-06-27.** All thirteen design items (A1–A13) are
> complete: every open question is resolved (the A11 table + A12 / A13 closed
> the last three), and the consolidated implementation tasks **B1–B10**
> (§ Phase B tasks — consolidated) are no longer a draft. Phase B **may begin**
> — one task at a time, in dependency order (B1 presence / B2 registry first),
> keeping `sl-client-tokio` / `sl-client-bevy` / the REPL at parity. **Ask the
> user before starting Phase B** (the standing "ask before new roadmap work"
> rule). The consolidation pass that merged the draft B1–B13 into B1–B10 (per
> the `PERMISSION_ROADMAP.md` precedent) is recorded in the note below.
>
> **Phase B consolidated — 2026-06-27.** The draft per-A-item tasks (B1–B13)
> were merged and reordered into the dependency-ordered **B1–B10** in § Phase B
> tasks — consolidated, below, to remove dead-code / rework **between** tasks
> (each task now adds every field/type together with its writer, its reader, and
> tests, so every intermediate commit is clippy-clean under the `unused_*` deny
> lints). The A1–A13 **reference** subsections are unchanged design records.
> Trap → fix summary: draft B1 was a dead type alone → folded into the registry
> (new B2); draft B2 was a dead store (no fold / no accessor) → merged with the
> create/track mechanics (new B2); draft B4's `Invited` variant had no
> constructor until draft B5 → the lifecycle enum is born with the invite task
> (new B5); fields pre-declared before their reader → each field now lands with
> its fold + accessor; `ChatMessage` introduced then renamed to `SessionMessage`
> → the rename is applied up front (new B4); the reverse-XOR
> `direct_peer_from_session_id` had no consumer → dropped; helpers introduced
> before their first caller → each lands with its consumer.
> **Old → new B-number remap** (re-pointed by meaning; old B4 splits): old B1·B2
> → B2; B3 → B1; B4 → B2 (mechanics) / B5 (lifecycle); B5 → B5; B6 → B3; B7 →
> B6; B8 → B4; B9 → B10; B10 → B7; B11 → B10; B12 → B8; B13 → B9.

## Phase B tasks — consolidated (B1–B10)

The draft per-A-item tasks (originally B1–B13) were merged and reordered into
the ten dependency-ordered tasks below to eliminate dead code / rework
**between** tasks: with `sl-proto`'s `[lints.rust]` denying the `unused_*`
family and the ggh pre-commit re-running full clippy on every attempt, an
intermediate commit that adds a field nothing reads, or an enum variant nothing
constructs, fails the gate. So each task adds every field/type **with** its
writer (fold / method), its reader (accessor / test), and tests, leaving the
tree buildable, clippy-clean (restriction lints), and `cargo test`-green on its
own. The reference subsections above are unchanged design records; the
`(was old B#)` tag on each task maps it back to the draft it absorbs (see the
remap table in the consolidation note).

Work these top-to-bottom; tick a box only when the step builds, is clippy-clean,
and `cargo test` passes. Keep `sl-client-tokio`, `sl-client-bevy`, and the REPL
at feature parity; never push client-only types into shared `sl-types`.
**Ask the user before starting Phase B** (the standing "ask before new roadmap
work" rule).
