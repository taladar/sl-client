---
id: test-audit-conformance-trace-correctness
title: sl-conformance-trace mislabels every datagram on loopback and correlates across circuits
topic: test
status: bugs
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
