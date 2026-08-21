---
id: viewer-im-tab-shows-short-id-not-name
title: An IM tab we open ourselves is titled with a short id, never a name
topic: viewer
status: done
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

## Fixed (2026-08-21)

The parallel cache is **gone**, and with it the reason it existed.

The first cut kept the model's own `agent_names` and merely fed it more events,
on the grounds that the shared cache is pruned. That was fixing the symptom: a
name is knowledge about a *person*, not about a presence, and
`AvatarState::purge` was dropping every name on a distant teleport because the
*region* changed. Most names this viewer shows are for avatars nowhere near it
— group members and group chat, an object's or parcel's owner and creator, an
inventory item's creator, an open conversation's peer — so that purge threw
away answers it would immediately have to ask for again. It cannot grow enough
to matter over a session, and if it ever did the bound would be
least-recently-used, not "is standing near me". So `names` (and the request
bookkeeping that keeps a resolved name from being re-requested) now survive the
purge, while everything genuinely scene-shaped still goes.

With that fixed, `ConversationModel::agent_names` is deleted. The model
contributes *to* the shared cache instead — the sender names the wire stamps on
messages, invitations and history lines land via a new
`AvatarState::note_legacy_name`, which fills only a name not already known so a
lookup reply is never overwritten by whatever a message was stamped with. Tab
titles and stored speaker names resolve through `AvatarState::shown_name_of`
(pseudonym, else display name, else legacy), so a tab now shows what the name
tag over the same head shows; `title()` answers only the short-id placeholder
the view resolves over.

And a tab now **asks**: `request_conversation_names` requests the name of every
one-to-one peer the shared cache cannot name, through the same batched
`request_name` every other surface uses (one entry in the frame's
`UUIDNameRequest` / `GetDisplayNames` batch, once per agent). It runs only on a
model change and takes its mutable borrow only when something is missing, so an
idle conversation list neither re-requests nor dirties the cache.

Group and conference titles were checked and left alone: a group tab is for a
group we are in, whose name arrives with the login membership list, and a
conference is named by its invite or — for one we started — the
`conversations-conference-title` string.

**Verified live on aditi**: one-to-one tabs are titled for the resident, and
names resolve through the one cache with nothing name-related in the log.
