---
id: viewer-objectupdate-truncated-block-drops-message
title: An ObjectUpdate from aditi fails to decode and every object in it is dropped
topic: viewer
status: done
origin: aditi live check of viewer-neighbour-object-caps-use-root-region (2026-09-17)
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

On aditi (the tutorial area around Ahern), shortly after login, two
`ObjectUpdate` messages (`High(12)`) were dropped whole:

```text
dropping undecodable inbound message id=High(12) name="ObjectUpdate"
  error=unexpected end of data: needed 4 more byte(s), had 3
protocol diagnostic: DecodeFailed ... failed_offset=1713
protocol diagnostic: DecodeFailed ... failed_offset=1578
```

One `ObjectUpdate` carries a batch of objects, so a decode failure anywhere in
it loses **all** of them, not just the object with the odd block. Firestorm
decodes the same stream, so either our template decode is stricter than
`LLMessageSystem` about a short trailing field or a variable block, or a
hand-written sub-decoder reads past its block.

## Investigate

- Capture the datagram: `sl-conformance-trace` over a `tcpdump` of an aditi
  session, or log the raw bytes of the undecodable message, and decode it
  offline to see which block and field is 3 bytes long at offset
  1578 / 1713.
- Compare with the reference's `LLMessageSystem` handling of that field: does
  it tolerate a short final variable block, or zero-fill it?
- Whatever the cause, consider whether one bad object block should cost the
  rest of the message.

## Cause (confirmed on aditi, 2026-09-17)

A `sl-repl-tokio --script` probe at Ahern (`set_diagnostics on`, a 60–150 s
hold) reproduced it on most logins, and a temporary log of the **encoded** body
settled it. Every failing datagram was `RELIABLE | ZEROCODED`, and every encoded
body ended in the same pair, `00 42`: a final run of **66** zeros. The fields it
covers need **67**: `ExtraParams = [00]` followed by the 66 bytes of `Sound`,
`OwnerID`, `Gain`, `Flags`, `Radius`, `JointType`, `JointPivot` and
`JointAxisOrAnchor`, all zero. So the body expands one byte short and the last
`F32` of the last object block has three bytes ("needed 4 more byte(s), had 3").
Our zero-run expansion is identical to the reference `zeroCodeExpand` for this
input. The **simulator** encodes the final run short.

Firestorm survives it because `LLTemplateMessageReader::decodeData` is lenient
past the end of a packet. It logs "Ran off end of packet" and **zero-fills** a
fixed field, gives a variable field length 0, and treats a missing `Variable`
block count as 0 repeats ("hetgrid says that missing variable blocks at end of
message are legal").

## Fix

- `sl-wire`: `Reader::with_zero_tail`. Reads past the end yield zero bytes and
  are counted (`Reader::zero_filled`). The generated template decoder reads
  variable fields through the new `variable1_owned` / `variable2_owned` and
  block counts through `variable_block_count`, so all of them honour it. A
  missing trailing block count is still a silent 0, as before.
- `sl-wire`: `ends_in_zero_run(encoded)`. The lenient reader is used **only**
  for a zero-coded body whose encoding ends in a zero run, the one shape the
  evidence supports. A message cut short anywhere else is still a
  `DecodeFailed`, as before.
- `Session::handle_datagram` (shared by the tokio and Bevy runtimes) and the
  `sl-conformance-trace` decoder use it. The session logs a `warn` naming the
  message and the number of bytes filled, so the leniency is never silent.
- Tests: reader/helper unit tests in `sl-wire`, and two `lifecycle` tests. One
  builds an `ObjectUpdate` encoded the simulator's way and checks its object
  arrives with no `DecodeFailed`. The other checks that a zero-coded message
  cut inside its literals still fails.
- Book: `comms/lludp-transport.md` § Zero-coding.

## Live verification

With the fix, the same aditi probe shows no `DecodeFailed` at all across four
logins, and a `zero_filled` warning where each drop used to be, with the
message's objects arriving. Almost all were `zero_filled=1`. **One** message
needed `zero_filled=37`, and it did not recur in the later runs, so its encoded
bytes were never seen. Its decoded objects looked sane (consistent linkset
parents, shapes and zero joint fields). Zero-filling only ever happens after all
the real data, so every earlier block decodes exactly as a strict reader would.
The result is the same message Firestorm's reader builds.
