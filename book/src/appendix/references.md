# References

Where to go for the authoritative details this book summarizes.

## In this repository

- **`message_template.msg`** (`sl-wire/message_template.msg`) — the canonical
  list of every LLUDP message, its frequency, trust, encoding, blocks, and
  fields. When in doubt about a UDP message's exact shape, this file is the
  source of truth. See [Messages & the Template](../comms/messages.md).
- **`roadmap/`** (repository root) — the planning tree: one small markdown file
  per task, sorted by status, with a generated `roadmap/INDEX.md` overview. The
  per-feature gap analysis and fidelity audit lives under the `protocol` topic
  (`roadmap/context/protocol.md` plus the `protocol-*` task files); use it as
  the feature-coverage matrix that complements this book's conceptual overview.
- **Crate `lib.rs` docs and per-crate `README.md`** — API-level documentation,
  generated with `cargo doc`. This book is the *conceptual* layer above them.
- **Examples** under `sl-client-tokio/examples/` — runnable end-to-end uses of
  the stack (login, inventory, profiles, groups, media, materials, voice, asset
  upload/fetch, object rez/edit, terrain probe).

## Upstream implementations (vendored alongside this workspace)

These reference server/viewer source trees are checked out next to the workspace
and are invaluable for resolving "what does the real implementation do here":

- **OpenSimulator** (`~/devel/3rdparty/opensim`,
  `~/devel/3rdparty/opensim-core`) — an open-source server implementation; the
  most readable source for how a region handles each message and capability.
- **Firestorm / the Second Life viewer** (`~/devel/3rdparty/phoenix-firestorm`)
  — an open-source viewer; the reference for client-side behaviour and for
  features OpenSim does not implement.

## Online

- **Second Life Wiki — protocol documentation**:
  <https://wiki.secondlife.com/wiki/Protocol> (message and capability
  references, LLSD, the login protocol).
- **mdBook user guide** (this book's tooling):
  <https://rust-lang.github.io/mdBook/>.

> A reminder from the [Introduction](../introduction.md): Second Life is the
> primary target and OpenSim the test grid. Where the two disagree, the Second
> Life behaviour is the one to match, and the OpenSim source is consulted for a
> working reference rather than as the definition.
