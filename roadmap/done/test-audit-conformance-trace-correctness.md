---
id: test-audit-conformance-trace-correctness
title: sl-conformance-trace mislabels every datagram on loopback and correlates across circuits
topic: test
status: done
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/test.md](../context/test.md).

Two correctness bugs in the offline protocol-trace tool, both fixable by keying
on `SocketAddr` instead of `IpAddr` — the port is already parsed at
`logfile.rs:63` and then discarded (`message.host.ip()`):

- `src/trace/timeline.rs:35-41` — `direction_of` is
  `if self.sim_ips.contains(&destination) || self.viewer_ips.contains(&source)`.
  With OpenSim at `127.0.0.1:9000`, `sim_ips = {127.0.0.1}` makes both branches'
  conditions true, so the first always wins and **every datagram is labelled
  `ViewerToSim`**. Broken on the project's primary test grid.
- `:131` — the correlation index is `HashMap<(Direction, u32, String), ...>`.
  LLUDP sequence spaces are **per circuit**, and a viewer holds a root plus
  child circuits at once, so an `AgentUpdate` seq 5 to region A and seq 5 to
  region B collide and the FIFO `pop_front` (`:185`) hands the wrong viewer
  timestamp to the wrong datagram.

Also `:141-155` — correlation runs over `datagrams` in **file order** and only
sorts by timestamp afterwards (`:155`), so a merged or multi-interface capture
assigns log timestamps to the wrong occurrence. Sort first, then correlate.

Robustness, same tool:

- `src/trace/pcap.rs:105-113` — `interface_kinds` is never reset on a Section
  Header Block, and `Block::SectionHeader` is not matched at all (`_ => {}` at
  `:124`). In pcapng the interface-ID space restarts per section, so every
  packet after the first section of a concatenated or rotated capture is peeled
  with the previous section's link type.
- `:91`, `:109` — a truncated capture (a Ctrl-C'd `tcpdump`) makes
  `next_packet()` return `Err`, which aborts the whole read and discards the
  thousands of datagrams already parsed. Stop and return the partial vector.
- `:74` — `fs_err::read(path)` slurps the entire capture into RAM;
  `PcapReader` accepts any `Read`, so a `BufReader<File>` costs nothing.
- `:187-189` — `udp_length` is recorded but never compared against
  `udp.payload().len()`, so a **snaplen-truncated** packet surfaces as a
  spurious `<PARSE ERROR>` indistinguishable from a real protocol divergence —
  the exact thing the tool exists to find.
- `src/trace/logfile.rs:75-95` — every field is `.parse().ok()?`, so format
  drift makes a line vanish; `read_log` reports no skip count, `run` never
  checks `log.messages.is_empty()`, and `report_summary` (`:142`) counts only
  *pcap* errors. A log the tool cannot parse yields a full timeline with zero
  `viewer_ts` and no indication anything went wrong.

`src/trace/pcap.rs` has **zero** tests; every item above is a small table test.

## Resolution

**An endpoint is an `ip:port`, and the better-evidenced direction wins.**
`Endpoints` now holds a `Side` per party, each an exact-`SocketAddr` set plus a
whole-IP set, and `label` scores both candidate directions by how specifically
each end matches (exact socket 2, IP 1, no match 0) instead of testing the
simulator side first. On the loopback grid, `127.0.0.1:9000` as the simulator
scores the datagram *to* 9000 and the reply *from* it differently, so the two
directions come out right; the log supplies those sockets already, because
`LogFile::sim_hosts` keeps the port `logfile.rs` had been parsing and throwing
away. `--sim-addr` / `--viewer-addr` accept either spelling
(`EndpointSpec: FromStr`).

**A tie is reported, not guessed.** When both ends match a side equally well —
a loopback capture named only by IP, which carries no directional information
at all — the datagram is dropped as `Labelling::Ambiguous` and the run summary
says so and names the fix (pass the port). Silently labelling all of them
`V->S`, as before, is the failure this task is about; inventing a different
arbitrary answer would only move it.

**Correlation is per circuit and in time order.** The index key gained the
simulator `SocketAddr`, so seq 5 on the root circuit and seq 5 on a child
circuit no longer share a FIFO queue, and `build_timeline` sorts the datagrams
by capture time *before* correlating rather than after, so a merged or
multi-interface capture hands each occurrence its own viewer timestamp.

**A damaged capture costs its damage, not the whole file.** Both readers stop
at a record that will not parse and return what they already have, with the
error in `Capture::stopped_early`; the pcapng reader clears its interface table
on a Section Header Block, so a rotated or concatenated capture's second
section is peeled with its own link types (and frames on an interface it cannot
peel are counted, not silently lost). The file is streamed through a
`BufReader` instead of being slurped whole.

**Snaplen truncation is a first-class outcome.** Frames are sliced with
etherparse's *lax* parser, so a datagram whose captured bytes stop short of the
length its headers declare still reaches the timeline, flagged
`UdpDatagram::truncated`, printed as `<TRUNCATED CAPTURE>` with a
`captured=NB` note, and carried as `udp.truncated` in the JSON — the one
distinction that matters in a tool whose job is telling "the capture stopped
short" apart from "the two implementations disagree". Live-testing this found a
further blocker the audit had not: `pcap_file`'s cooked `PcapPacket` rejects any
record whose original wire length exceeds the file's snaplen, which is what
*every* record of a `tcpdump -s <n>` capture looks like, so the first one
aborted the read. The classic-pcap path now reads raw records and builds the
timestamp itself from `ts_sec`/`ts_frac` and the file's declared resolution.

**A log the tool cannot read says so.** A line whose `MSG:` marker is followed
by a direction arrow but whose fields do not parse is counted in
`LogFile::skipped_lines` rather than vanishing, and the binary warns on that and
on a log with no `#Messaging#` lines at all (the `LogMessages` debug setting not
enabled) — previously such a log produced a full timeline with zero viewer
timestamps and nothing saying why. The run summary reports everything that did
*not* make the timeline: datagrams dropped as ambiguous or non-circuit, frames
skipped, datagrams cut short, and a capture that ended mid-record.

Verified by 18 unit tests across the three modules (`pcap.rs` had none) plus a
CLI run over synthetic loopback and snaplen-truncated captures: the loopback
capture labels `V->S` then `S->V` where the old code labelled both `V->S`, and
the short capture reads at all where the old code rejected it outright.
