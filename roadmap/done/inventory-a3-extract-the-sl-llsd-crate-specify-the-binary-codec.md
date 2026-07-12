---
id: inventory-a3
title: Extract the sl-llsd crate & specify the binary codec
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

**A3. Extract the `sl-llsd` crate & specify the binary codec.** Pull the
    LLSD core (`Llsd` + the XML codec + the notation reader now in
    `material/gltf.rs`) out of `sl-wire` into a new foundational `sl-llsd`
    workspace crate (depending only on `sl-types` + `uuid` + `base64` +
    `roxmltree` + `time`), with a crate-local `LlsdError` (and
    `From<LlsdError> for WireError` back in sl-wire) and a
    `pub use sl_llsd as llsd` re-export so the ~24 dependents keep compiling.
    Then add the new binary codec there: `Llsd::to_llsd_binary() -> Vec<u8>`
    and `parse_llsd_binary(&[u8]) -> Result<Llsd, LlsdError>` against the LL
    binary-LLSD tag bytes (`!` undef, `1`/`0` boolean, `i` i32 BE, `r` f64 BE,
    `s`+len+utf8 string, `u`+16 bytes uuid, `d`+f64 date, `l`+len+utf8 uri,
    `b`+len+bytes binary, `[`+count array, `{`+count map with `k`+len+key
    entries). The gzip envelope wraps the whole file *including* the 4-byte
    version header (Firestorm `saveToFile` writes header + binary LLSD to a
    temp file, then gzips it). Round-trips every `Llsd` variant and the real
    cache map. → see § `sl-llsd` extraction & binary-codec reference (from
    A3).

## `sl-llsd` extraction & binary-codec reference (from A3)

**Extraction first.** Create a new `sl-llsd` workspace member holding the LLSD
core: the `Llsd` enum, the XML codec (`to_llsd_xml` / `parse_llsd_xml`), the
notation reader currently in `sl-wire/src/material/gltf.rs`, and the typed-key
convenience accessors. It depends only on `sl-types`, `uuid`, `base64`,
`roxmltree`, and `time` — sitting **above** `sl-types` and **below** `sl-wire`
in the graph (no cycle). The current `sl-wire/src/llsd.rs` imports
`crate::error::WireError`, so the move introduces a crate-local `LlsdError` and
an `impl From<LlsdError> for WireError` in sl-wire. sl-wire keeps a
`pub use sl_llsd as llsd` re-export (and re-exports the moved free functions) so
the 20 sl-wire modules and the downstream `sl-proto` / runtime crates compile
unchanged; an ast-grep import sweep can later drop the shim. The LLSD-to-domain
converters that need sl-wire types (e.g. the inventory/material parsers)
**stay** in sl-wire.

**Binary codec** (added in the extracted crate, `sl-llsd/src/binary.rs`):
`Llsd::to_llsd_binary(&self) -> Vec<u8>` and
`parse_llsd_binary(bytes: &[u8]) -> Result<Llsd, LlsdError>`. Marker bytes
(all multi-byte integers big-endian / network order): `!` undef; `1` / `0`
boolean; `i` + 4-byte `i32`; `r` + 8-byte `f64`; `u` + 16 raw uuid bytes; `b` +
4-byte len + raw bytes (binary); `s` + 4-byte len + UTF-8 (string); `l` + 4-byte
len + UTF-8 (uri); `d` + 8-byte `f64` epoch-seconds (date); `[` + 4-byte count +
values + `]` (array); `{` + 4-byte count + count×(`k` + 4-byte len + UTF-8 key +
value) + `}` (map). Two wrinkles: (1) our `Llsd::Date` holds an ISO-8601
*string* but binary date is an `f64` epoch-seconds — the codec converts both
ways (reuse the `time`-based date handling in `llsd.rs`); (2) Firestorm writes
the trailing `]` / `}` — emit them for cross-readability and tolerate them on
parse. The cache envelope (A4) is `gzip(` 4-byte BE `u32` version `5`
`++ to_llsd_binary(map) )`.

**Boundary verified against the code (anchors for B1).** `sl-wire/src/llsd.rs`
(1318 lines) is **not** all LLSD core — it interleaves the generic value model
with sl-wire-specific CAPS builders that **stay** in sl-wire. Moves to
`sl-llsd`: the `Llsd` enum (11 variants, `:18`); the pure accessors `get` /
`index` / `as_array` / `as_map` / `as_str` / `as_i32` / `as_f64` / `as_f32` /
`as_bool` / `as_uuid` / `as_binary` / `kind` (`:47`-`:165`); the `field_*` /
`require_*` field accessors (`:178`-`:408`); the XML codec `to_llsd_xml`
(`:423`, infallible) / `parse_llsd_xml` (`:519`,
`-> Result<_, roxmltree::Error>`) with `node_to_llsd` / `push_llsd_xml`; and
`push_escaped` (`:593`, today `pub(crate)` — make it `pub` in `sl-llsd`, the
GLTF notation emitter needs it). Stays in sl-wire (sl-wire-typed, `WireError` /
keys): every `build_*` CAPS request (`build_seed_request` `:609` …
`build_fetch_inventory_request` `:637` … the `build_object_media_*` trio
`:1079`-`:1113`, `build_event_queue_*` `:1259`), the response types
`AssetUploadResponse` (`:935`) / `ObjectMediaResponse` (`:1157`) /
`EventQueueResponse` (`:1230`) with their `from_llsd`, `parse_seed_response`
(`:1213`), and the private `llsd_bool` / `llsd_int` / `llsd_string` /
`llsd_perm` / `llsd_uuid` helpers (`:1013`-). The three `sl_types` keys imported
at `:12` (`InventoryFolderKey` / `InventoryKey` / `ObjectKey`) are used **only**
by these staying builders, so the moved core's `sl-types` dependency is light
(retained per the locked decision for the typed accessors a future caller may
add).

**The re-export is a real module, not a crate alias (B1).** Because the
sl-wire-specific builders keep living in `sl-wire/src/llsd.rs`, that file stays
a real `crate::llsd` module that opens with
`pub use sl_llsd::{Llsd, parse_llsd_xml, push_escaped, …}` (re-exporting the
moved core) **and** keeps defining the builders — so both `crate::llsd::Llsd`
and `crate::llsd::build_seed_request` keep resolving at the **20** sl-wire
modules (verified count) and the downstream `sl-proto` (4 files) /
`sl-client-tokio` (7) / `sl-client-bevy` (7) call sites, all unchanged. A bare
`pub use sl_llsd as llsd` would leave the builders homeless.

**`WireError` coupling → `LlsdError` (B1).** The `field_*` / `require_*`
accessors return `Result<_, WireError>` today via two variants only —
`WireError::MalformedField { field: &'static str, value: String }`
(`error.rs:83`) and `WireError::MissingField { field: &'static str }`
(`error.rs:95`). The orphan rule forbids leaving `impl Llsd` in sl-wire once
`Llsd` is foreign, so these accessors **move** and re-type to a crate-local
`LlsdError` mirroring those two variants. **Implemented differently (see B1):**
rather than mapping `LlsdError` back onto generic `WireError::MalformedField` /
`MissingField`, those two `WireError` variants were **removed** — LLSD faults
now flow through a transparent `WireError::Llsd(#[from] LlsdError)`, the
non-LLSD text-scalar case moved to a new `WireError::InvalidScalar`, and every
sl-wire LLSD parse site produces/propagates `LlsdError` rather than
constructing `WireError`. `parse_llsd_xml` keeps its `roxmltree::Error`, so it
moves clean.

**Notation reader is GLTF-entangled (B1).** `material/gltf.rs` mixes a generic
notation-LLSD cursor (`:59`-~`:300`: string/int/array token readers, "advance
past one value") with GLTF-domain decode (`modify_material_update` `:365`,
`-> Result<_, WireError>`, `GltfMaterialOverride`). B1 moves **only** the
generic cursor primitives to `sl-llsd`; the GLTF-typed decode stays in sl-wire.
If the cursor proves too entangled with GLTF byte-span semantics, B1 may keep it
in sl-wire — the A3 deliverable (the *binary* codec) is independent of it.

**Binary codec confirmed against Firestorm (anchors for B2).** Ground-truthed in
`indra/llcommon/llsdserialize.cpp` (`LLSDBinaryFormatter::format_impl` `:1541`,
`LLSDBinaryParser::doParse` `:952` / `parseMap` `:1186` / `parseArray` `:1240`)
and `newview/llinventorymodel.cpp` (`saveToFile` `:3779`, `loadFromFile`
`:3661`, `sCurrentInvCacheVersion = 5` `:97`). Tags are exactly as A3 states.
Newly pinned wrinkles B2 must honour:

- **Closing `]` / `}` are mandatory, not decorative.** `parseArray` / `parseMap`
  return `PARSE_FAILURE` if the terminator is absent — so emit them **and**
  require them on parse. The 4-byte BE count prefix is authoritative: parse
  exactly `count` entries then expect the terminator (a mismatch is an error).
- **Date endianness is asymmetric in Firestorm.** `format_impl` writes `Real`
  through `ll_htond` (network/BE) but writes `Date`'s `f64` *raw* with no swap
  (host-endian), read back raw — so Firestorm dates are host-endian, unlike
  every other multi-byte field. **But inventory caches never hit this:** item
  creation dates serialise as LLSD `Integer`
  (`LLSD::Integer(item->getCreationDate())`), not LLSD `Date`, so the cache map
  carries no `Date`. B2 still matches Firestorm for general round-trip
  (host-endian date, or document the divergence); the agent/library cache is
  unaffected either way.
- **Our `Llsd::Date` holds an ISO-8601 *string*** (`:33`, verbatim; the XML
  codec does no `time` parsing) while binary date is `f64` epoch-seconds — so
  the binary date path is the one place needing `time` (ISO ↔ epoch). `time` is
  therefore a **B2** dependency, not B1.
- **Parser tolerates notation-style strings.** `doParse` / `parseMap` also
  accept `'` / `"`-delimited string values and quoted map keys as a fallback;
  our parser tolerates them on read but only ever **emits** the length-prefixed
  `s` / `k` forms.
- **File framing (A4 cross-ref).** `saveToFile` writes `htonl(5)` (4-byte BE),
  then one binary-LLSD map `{ "categories": [...], "items": [...] }` via
  `LLSDOStreamer`, **then a trailing `\n`** (`<< std::endl`), then gzips the
  whole temp file separately (`gzip_file`). So the gzip envelope wraps
  header+map+newline; our writer appends the `\n` (harmless) and our reader
  tolerates trailing bytes after the top-level map. `loadFromFile` reads the
  4-byte version first and treats any value `!= 5` as obsolete (ignored). The
  `getVersion() != VERSION_UNKNOWN` save filter is the Firestorm anchor for
  A10's "`Loaded` folders only" snapshot.
