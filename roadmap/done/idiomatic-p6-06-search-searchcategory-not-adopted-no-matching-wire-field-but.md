---
id: idiomatic-p6-06
title: search::SearchCategory — **NOT ADOPTED** (no matching wire field), but
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 6 — Adopt `sl-types` non-key value types (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`search::SearchCategory` — **NOT ADOPTED** (no matching wire field), but
    did the genuine adjacent hardening it pointed at: a new local
    `ClassifiedCategory` enum. `sl_types::search::SearchCategory`
    (`All`/`People`/`Places`/`Events`/`Groups`/`Wiki`/`Destinations`/
    `Classifieds`) is the *Search-floater tab* /
    `secondlife:///app/search/ <category>` viewer-URI concept; no single LLUDP
    field carries it (the directory queries express it implicitly through
    *which* message is sent — `DirFindQuery`+flags / `DirPlacesQuery` /
    `DirClassifiedQuery` / `DirLandQuery` — and via the web-search CAP). The
    queries are not even uniform in shape (Places adds a
    `ParcelCategory`+region filter, Land drops `query_text` and adds
    sale-type/price/area and has no `SearchCategory` variant at all, three
    variants — All/Wiki/Destinations — have no UDP query whatsoever), so a
    `SearchCategory`-dispatched API would buy nothing over the typed `Command`
    variants. Same situation as `viewer_uri::ViewerUri` / `map::Location` /
    `map::ZoomLevel` (see considered-not-adopted). The roadmap's parenthetical
    was right: parcel category → already `ParcelCategory`,
    `EventInfo.category` → free-text `String`. **The actual raw `category:
    u32` directory fields are the *classified-ad* category**
    (`Any=0, Shopping=1, Land Rental=2, … Personal=9` — the viewer's
    `panel_dir_classified.xml` combo, a *different closed code set*, not a
    `SearchCategory`), so (user decision: "Reject + ClassifiedCategory") added
    a **new public client-local `ClassifiedCategory` enum** in
    `sl-proto/src/types/avatar_profile.rs` next to `ClassifiedInfo` (mirroring
    `ParcelCategory`: `#[non_exhaustive]`, `#[default] AnyCategory`,
    `Unknown(u32)`, `from_u32`/`to_u32`). Typed every classified-category
    field: `ClassifiedInfo.category`, `ClassifiedUpdate.category`,
    `Command::DirClassifiedQuery.category`,
    `ServerEvent::DirClassifiedQuery.category`, and the
    `Session::dir_classified_query` param. Codec wraps at the boundary (decode
    `ClassifiedCategory::from_u32` in `conversions.rs` / `sim_session.rs`,
    encode `.to_u32()` in `circuit.rs`) so the `Category` U32 wire word is
    byte-identical. **Kept client-local in `sl-proto` (NOT `sl-types`)** per
    the standing rule (new types go local first, batch-migrated later to avoid
    version churn) — same precedent as `LandArea`/`LindenBalance`/the union
    keys. Left raw (deliberately, a different concept): the *object* category
    code (`ObjectProperties`/`ObjectPropertiesFamily.category`,
    `Command::SetObjectCategory`), `EventInfo.category` (free text), the abuse
    `category` (u8). Re-exported `ClassifiedCategory` through
    `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity; the runtimes
    forward the typed `Command` field verbatim). REPL
    `build_classified_update` / the `dir_classified_query` command parse the
    raw `u32` then wrap; the `profile_edit` example builds
    `ClassifiedCategory::Shopping` and prints via `.to_u32()`. Book
    `content/search.md` updated. +1 focused unit test
    (`classified_category_round_trips_raw_u32`: every named code ⇄ wire value,
    `Unknown` verbatim, default = `AnyCategory`/`0`); lifecycle +
    `sim_session` round-trip suites updated. Build + clippy
    (`--workspace --all-targets`) + tests + `cargo doc` (`-D warnings`) +
    mdbook green. NO `sl-types` touched.
