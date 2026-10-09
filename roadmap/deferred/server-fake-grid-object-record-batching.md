---
id: server-fake-grid-object-record-batching
title: Fake grid — send object records several to a message, as each grid does
topic: server
status: deferred
origin: gridspec-object-properties (2026-10-09)
refs: [gridspec-object-properties, server-fake-grid-object-update-forms,
  server-fake-grid-object-record-texture-ids]
---

Context: [context/server.md](../context/server.md).

**Deferred until it matters.** Implement this when a test needs it or a bug
cannot be reproduced without it — a client that mishandles a multi-record
`ObjectProperties`, or one whose behaviour depends on records arriving
spread over several ticks. Until then nothing above the wire codec can tell
the difference: the client emits one `Event::ObjectProperties` per record
however they were grouped.

The fake grid answers a select with one `ObjectProperties` message per
object (`SimSession::send_object_properties` takes a single record). Neither
live grid does (`object-properties` and `object-select-scene`, 2026-10-09,
`book/src/gridspec/objects.md` § Properties):

- **OpenSim** turns queued records into messages at most every 20 ms
  (`LLUDPClient.MIN_CALLBACK_MS`), each time as many as the task throttle
  lets through in 30 ms (`LLClientView.HandleQueueEmpty`), charging a record
  a flat 200 bytes (`ProcessEntityPropertyRequests`), and splits one pass's
  records at 1,200 bytes (`LLUDPServer.MAXPAYLOAD`). At the default task
  rate of 18,500 bytes a second that is three records a message: a select
  of eight came back as three, three and two, 15 to 30 ms apart. A client
  that sets a higher task throttle is sent more to a message.
- **Second Life** sent eight records in two messages in the same
  millisecond, split four and four in one run and five and three in
  another. The rule was not found. Its records carry a texture id per face
  ([[server-fake-grid-object-record-texture-ids]]), so their sizes differ.

## To do, when it is picked up

- A sender for several records in one `ObjectProperties`.
- A `PropertiesPolicy` row for how records are grouped. OpenSim's needs the
  session's task throttle and a tick to release a pass on; the fake grid has
  neither for this path today.
- For Second Life, measure first: two selects a few milliseconds apart (do
  their records share a message?) and one select of forty objects (where is
  the cap, and how are the messages spaced?).
- The same question for the object-update stream is
  [[server-fake-grid-object-update-forms]]'s; share the mechanism.
