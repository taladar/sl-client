---
id: test-offline-msg-fetch-aditi
title: Offline message fetch — [aditi] variant
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

The `[aditi]` variant of the `offline-msg-fetch` case
(`[[test-offline-msg-fetch]]`, green `[opensim]`): **pass (partial) on
aditi live** (2026-08-12, Phase Z). The store-and-forward *write* is
proven on Second Life; the in-world *read-back* is a documented gap.

Per-grid routing was added: the stored-reply wait accepts both grids'
wording (OpenSim "Message saved.", SL "User not online - message will be
stored and delivered later."), and the fetch branches (OpenSim UDP
`RetrieveInstantMessages`; SL the `ReadOfflineMsgs` capability, waited for
after the relogin and retried).

**Second Life read-back gap (established live):** the write half always
round-trips — the sender receives SL's "will be stored" reply — but the
recipient's in-world `ReadOfflineMsgs` store stays **empty** across a
generous retry budget (nine clean GETs over 90 s in the diagnosis run,
each decoding to zero messages), and no offline IM is auto-delivered
after login either. Second Life routes offline IMs to email / gates
in-world retrieval on account state the harness cannot set, so the
read-back is not observable here. The case records the write as proven
(`store_confirm` timing) and marks the read-back a legitimate partial
(`read_back_observed = false`) rather than failing — the mirror of
`avatar-notes`, whose SL read-back is likewise unobservable. Not a client
bug: the `ReadOfflineMsgs` GET, its LLSD decode, and the UDP fallback all
work; the store is simply empty. A future full SL run would need an
account configured for in-world offline delivery (offline-IM-to-email
off) or a longer settling window than the login cooldown allows.
