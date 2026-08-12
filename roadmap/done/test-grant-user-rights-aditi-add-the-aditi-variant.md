---
id: test-grant-user-rights-aditi
title: Grant user rights — [aditi] variant
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

The `[aditi]` variant of the `grant-user-rights` case
(`[[test-grant-user-rights]]`, already green `[opensim]`): **green on aditi
live** (2026-08-12, Phase Z batch) after one behaviour finding.

**Second Life masks `CAN_SEE_ONLINE` out of every grantee-visible rights
view.** The primary grants the full see-online | see-on-map |
modify-objects set (7); the grantor-side echo carries 7 on both grids, but
SL's `ChangeUserRights` notify to the *grantee* and the grantee's
buddy-list `rights_received` both carry 6 — the online-visibility bit is
private to the grantor's side there, while OpenSim relays the full
bitfield everywhere. The case now hard-asserts only the granted map/modify
bits in grantee-visible views and records whether the see-online bit was
included (`notify_included_online` / `received_included_online` metrics:
true on OpenSim, false on SL).
