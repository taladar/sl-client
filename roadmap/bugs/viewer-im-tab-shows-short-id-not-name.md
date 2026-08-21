---
id: viewer-im-tab-shows-short-id-not-name
title: An IM tab we open ourselves is titled with a short id, never a name
topic: viewer
status: bugs
origin: seen on aditi while live-checking [[viewer-conference-start-ui]]
  (2026-08-21)
refs: [viewer-social-im-conversations, viewer-social-profiles,
  viewer-avatar-radar]
---

Context: [context/viewer.md](../context/viewer.md).

Opening a one-to-one IM from a profile gives a tab labelled with a short id
instead of the resident's name — even though the viewer had just displayed that
name in the profile it was opened from.

## Why

`ConversationModel`'s name cache is fed **only from inbound chat traffic**.
Every `note_agent_name` call site in `conversations.rs` is an *arriving* IM, a
conference invitation, or a session-history line; the model never reads the
viewer's shared name sources. `ConversationModel::title` therefore falls back to
`short_id(id.uuid())` for any conversation the **user** opened, and only learns
the name if the peer happens to say something.

So the name is known — twice over — and simply not consulted:

- `AvatarState::name_of` (the batched `RequestAvatarNames` cache every other
  surface reads: the radar, name tags, the profile),
- `FriendsModel::name_of` for a friend,

and the arriving `SlSessionEvent::AvatarNames` / `DisplayNames` replies are not
folded into `agent_names` either.

## Fix

Resolve a `Direct` tab's title through the shared name cache rather than a
private one: fold `AvatarNames` / `DisplayNames` into `agent_names` on ingest
**and** request the name when a tab opens for an agent whose name is unknown
(the same one-shot `Command::RequestAvatarNames` the radar and the profile
issue). Keep the short-id text as the placeholder *until* the reply lands, as
the reference does — it is a fallback, not a title.

The group and conference cases have the same shape (`group_names` /
`conference_names` are also invite-fed) and should be checked while in there: a
group tab opened from the group profile, or the conference tab this viewer
itself started, should be named without waiting for someone to speak.

## How to verify

Open an IM from a profile / radar row for a resident who has said nothing: the
tab must show their name (after the name reply, if it was not already cached),
not `a1b2c3…`.

Reference (Firestorm, read-only): `llimview.cpp` (`LLIMModel::LLIMSession`
takes the session name at construction, from `LLAvatarNameCache`).
