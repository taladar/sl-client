---
id: viewer-group-notice-display
title: Group notice display — image, title, body, item toast
topic: viewer
status: done
origin: user request (2026-07-28), the received-notice popup the group profile
  Notices tab did not cover
blocked_by: [viewer-ui-notification-host, viewer-social-group-profile]
refs: [viewer-group-notice-attachments, viewer-url-linkification]
---

Context: [context/viewer.md](../context/viewer.md).

When a group posts a notice, every member with notices enabled receives it as an
`IM_GROUP_NOTICE` instant message. The group profile's Notices tab
([[viewer-social-group-profile]]) shows a notice a member **pulls up** to read,
but nothing displayed an **unsolicited** notice push. This task adds the
received-notice popup — the reference `LLToastGroupNotifyPanel`: a stacked card
with the group **image** (insignia), a **"Group Notice"** header and **"Sent by
…"** title, the **subject** + posting date + **body**, and the attached **item**
(icon + name) when the notice carries one, plus **OK** / **Group Notices** /
**Group Chat** actions and a close ×.

Reference (Firestorm, read-only): `lltoastgroupnotifypanel`,
`panel_group_notify.xml`, the `IM_GROUP_NOTICE` decode in `llimprocessing.cpp`.

## Done (2026-07-29)

- **Decode** — `InstantMessage::group_notice()` in `sl-proto` decodes the
  `subject|body` message and the `notice_bucket_full_t` binary bucket
  (`has_inventory`, asset type, group id, item name) into a
  `GroupNoticeReceived` (group / sender / subject / body / timestamp / optional
  `GroupNoticeItem`). Unit-tested (attachment / no-attachment, pipe-in-body,
  truncated-bucket fallback to the sender id, dialog gating).
- **Model** — `GroupsModel` now retains each member group's insignia texture
  from the login `AgentGroupDataUpdate`, so the toast can show the notice's
  group image.
- **Host** — new `group_notice.rs` + `GroupNoticePlugin`: pops a card per
  received notice, requests + swaps in the insignia texture, and wires OK
  (dismiss) / Group Notices (`OpenGroupProfile`) / Group Chat
  (`StartGroupSession` + `OpenConversation`). The card is
  **adopted into the shared notification-host channel**
  (`notification_host::adopt_toast`) rather than a channel of its own, so it
  stacks top-right (mirroring under RTL), orders by priority, and joins the
  **"N more ▸" overflow cycling** with the catalogue toasts instead of filling
  the screen edge.
- **Seen only on active close** — the card is an `Alert` (never auto-fades);
  display sends **no** server acknowledgement, and overflow only *hides* a
  queued notice. It ends only when the user clicks OK / × (a
  `ResolveNotification` teardown that is a pure UI dismissal — no `SlCommand`),
  so an unclosed notice can be redelivered by the server on the next login.

**Note for the attachment follow-up ([[viewer-group-notice-attachments]]):**
when close-on-dismiss learns to **decline** an unaccepted attachment
(`IM_GROUP_NOTICE_INVENTORY_DECLINED`), that server message must be gated to a
**freshly-received** notice with a *live* inventory offer — never a notice
re-opened from history / the Notices tab (whose offer is long gone). Because the
current close path emits no `SlCommand` at all, a future "re-open a past notice
as a card" affordance is safe by construction; only the fresh-offer decline
needs this gate.

- **Notices-tab coordination** — the tab records the notice ids it requested in
  `RequestedGroupNotices`; the host suppresses the toast for those (the
  reference `IM_GROUP_NOTICE_REQUESTED` vs. a fresh `IM_GROUP_NOTICE`
  distinction).
- **SLT timestamps** — the posting date renders in Second Life Time (US Pacific,
  the zone notices are written in) via the status bar's `slt` conversion, marked
  `SLT`.

**Deferred (follow-ups):**

- **Body links** — the body is plain text; turning its URLs / SLURLs into
  clickable links is [[viewer-group-notice-body-links]], on the shared
  linkification layer ([[viewer-url-linkification]]).
- **Attachment accept** — the card **shows** the attached item but does not yet
  copy it into inventory; the receive-side accept is
  [[viewer-group-notice-attachments]].

**Verification:** the decode is unit-tested; the panel is verified live (a group
posts a notice; the member's viewer pops the card) — group notices need a live
grid, so the rendering is checked by running the viewer, not a headless test.
