# sl-llsd

LLSD (Linden Lab Structured Data) value model and codecs, shared across the
Second Life / OpenSim tooling in this workspace.

This crate holds the generic LLSD core, free of any wire-protocol or session
types:

- the [`Llsd`] value enum and its pure accessors (`get` / `index` / `as_*` /
  `kind`),
- the typed field accessors (`field_*` / `require_*`) that read a map member of
  a specific LLSD kind, returning [`LlsdError`] on a missing or wrong-kind
  field,
- the LLSD-XML codec (`to_llsd_xml` / `parse_llsd_xml`) plus the `push_escaped`
  XML-escaping helper, and
- a minimal notation-LLSD cursor ([`Scan`]) for walking notation byte streams.

It sits above `sl-types` and below `sl-wire` in the dependency graph: `sl-wire`
keeps the capability (CAPS) request/response builders that depend on its own
`WireError` and the typed `sl-types` keys, and re-exports this crate's core
through its `llsd` module so existing call sites compile unchanged.
