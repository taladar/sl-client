---
id: chat-b1
title: Friend-presence cache (buddy list + online set)
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

## B1. Friend-presence cache (buddy list + online set) — DONE 2026-06-27

*(was old B3 — from A3.)* Fully standalone (no registry dependency); first
because it is clean on its own and B6 consumes it. See § Friend-presence
reference (from A3).

- [x] Add `friends: BTreeMap<FriendKey, Friend>` + `online: BTreeSet<FriendKey>`
  to `Session` (`session.rs`), const-empty in `Session::new`.
- [x] Seed `friends` at the `FriendList` site (`methods.rs:1078`) from the same
  `friend()`-mapped data; leave `online` empty at login.
- [x] Fold each existing handler (record in addition to emitting its event):
  `OnlineNotification` (`:3504`) inserts into `online`; `OfflineNotification`
  (`:3514`) removes; `ChangeUserRights` (`:3524`) updates the cached `Friend`'s
  rights by `granted_to_us` (ignore if absent); `TerminateFriendship` (`:2586`)
  removes from both stores.
- [x] Live friendship add (both directions): on `ImDialog::FriendshipAccepted`
  in the inbound IM dispatch, insert `from_agent_id` with default
  `CAN_SEE_ONLINE` both ways; add a `friend_id: FriendKey` field to
  `Command::AcceptFriendship` + a param to `accept_friendship`, inserting the
  friend on accept with the same default. Wire the changed command 6-site
  (tokio / bevy / REPL) at parity.
- [x] Accessors `friends()` / `friend(id)` / `is_online(id)` /
      `online_friends()` returning public `Friend` / `bool` / `FriendKey`.
- [x] Invariant: no IM / chat-session path mutates `online`; assert it (deliver
  an IM after an `OfflineNotification`; the peer stays offline). Unit tests for
  every handler above (seed, online/offline, rights by direction, unknown-friend
  ignored, terminate drops both, both live-add paths).

**Deviation (needs review):** the design assumed `FriendKey` was `Ord` (it is
the `BTreeMap`/`BTreeSet` key), but the sl-types key newtypes derive no `Ord`.
Rather than the local-`Ord`-wrapper workaround (`ScriptHolder` precedent —
impossible for a *bare* foreign key), added `PartialOrd, Ord` derives to `Key`
and `FriendKey` in the shared **sl-map-tools/sl-types** crate (purely additive).
This honours the roadmap-literal `BTreeMap<FriendKey, Friend>` and the
newtype-over-raw preference; B2 will need the same on `AgentKey` / `GroupKey` /
`ImSessionId` for `ChatSessionKind: derive(Ord)`.
