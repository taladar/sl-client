---
id: viewer-chat-sender-name-links
title: Chat sender names — clickable agent / object links
topic: viewer
status: done
origin: requested during viewer-url-linkification (2026-08-09) — the speaker's
  name in a chat line should be a link, like the reference
blocked_by: [viewer-url-linkification]
refs: [viewer-chat-history-panel, viewer-slurl-parse-dispatch]
---

Context: [context/viewer.md](../context/viewer.md).

In the reference viewer a chat line's **speaker name** is itself a link: a
resident name is a `secondlife:///app/agent/<uuid>/about` link (click → profile,
the `LLUrlEntryAgent` styling), and an object that speaks is a
`secondlife:///app/objectim/<uuid>?name=...&owner=...&slurl=...` link
(`LLUrlEntryObjectIM`, click → the object inspector). Our chat surfaces —
the nearby-chat overlay ([[viewer-url-linkification]]'s sibling
`chat.rs`) and the Conversations transcript ([[viewer-chat-history-panel]]) —
render the speaker name as **plain text** today.

Do: when a chat line is built, wrap the sender name as the matching app link
before the `": "` separator, so the name resolves / tints / clicks through the
shared linkification widget ([[viewer-url-linkification]],
`crate::linkified_text`) exactly like an inline `secondlife:///app/agent/...`
link would. An avatar speaker (`ChatSource::Agent`) gets an agent link; an
object speaker (`ChatSource::Object`) gets an objectim link carrying the owner
and region from the `ChatMessage`. The click dispatch for the SLURL is
[[viewer-slurl-parse-dispatch]]'s job.

Note the overlay currently formats each line as one joined string
(`format_chat_line`) and the transcript as one big string
(`format_transcript`); both need to move to the segment-rendered widget for the
name to become its own clickable run — the same conversion the chat / notice
body-link tasks make for the message body.

## Outcome (2026-08-09)

Done for the **Conversations transcript** only. The transient nearby-chat
overlay (`chat.rs`) is **deliberately left plain**: a fading heads-up line that
ate clicks would block picking in-world objects behind it, so its speaker name
stays non-interactive (the persistent transcript is where clicking belongs).

The transcript moved from one joined-string `Text` node to a **column of
per-line linkified rows** (`crate::linkified_text::spawn_linkified_text`),
rebuilt on a revision change. Each `TranscriptLine` carries a `SpeakerLink`
(agent / object / own / none), and `line_text` builds the line as a labelled
link `[secondlife:///app/agent/<id>/about  Name]: body` (an objectim link for an
object speaker), so the name shows plainly but targets the SLURL and the body's
own URLs / SLURLs linkify too. Recalled (persisted) history has no typed id, so
those names are not links. The SLURL click dispatch is
[[viewer-slurl-parse-dispatch]]'s job; web links in a line already open.
