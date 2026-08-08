---
id: viewer-chat-sender-name-links
title: Chat sender names — clickable agent / object links
topic: viewer
status: ready
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
