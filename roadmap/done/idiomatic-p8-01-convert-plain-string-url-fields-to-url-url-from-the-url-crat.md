---
id: idiomatic-p8-01
title: Convert plain String URL fields to url::Url from the url crate (use ur
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 8 — typed URLs (`url::Url`)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

Convert plain `String` URL fields to `url::Url` from the `url`
    crate (use `url::Url` directly — do NOT invent a client-local or
    `sl-types` URL wrapper). Fields whose empty string means "absent" become
    `Option<url::Url>` (empty/absent → `None`); a non-empty but unparsable
    URL is a hard decode error (the same non-masking stance as Phase 7 C: a
    present-but-malformed value is rejected, not kept raw). Candidate fields
    include the `OpenSimExtras` URLs (`map_server_url`, `search_server_url`,
    `destination_guide_url`, `avatar_picker_url`, `grid_url`), the
    `MediaEntry` `current_url`/`home_url`, the voice/seed capability URLs, and
    any other `String` field documented as a URL. **DONE 2026-06-24.** Added
    `url = "2.5.8"` to every crate that names the type (sl-wire, sl-proto, the
    two runtimes, sl-repl, the two REPL bins, sl-survey); a new
    `WireError::InvalidUrl { field, value }` and a `sl-wire/src/url.rs` codec
    boundary (`url_from_wire`/`optional_url_from_wire` +
    `url_to_wire`/`optional_url_to_wire`, mirroring `region_name`) carry the
    empty→`None`, non-empty-unparsable→hard-error convention; wire bytes are
    byte-identical (`Url::as_str` round-trips). **Converted** (all genuine,
    viewer-interpreted URLs): `OpenSimExtras` 5 URLs, `MediaEntry`
    `current_url`/`home_url`, voice `ParcelVoiceInfo.channel_uri` +
    `VoiceAccountInfo.account_server_name` (only the two that are real URIs —
    the SDP/session/type/hostname/credentials voice fields stay `String`),
    `LandResourcesUrls` summary+details, `ExperienceInfo`/`ExperienceUpdate`
    `slurl` (the `secondlife://` scheme parses fine), parcel
    `ParcelInfo`/`ParcelUpdate` `music_url`/`media_url` +
    `ParcelMediaUpdateInfo.media_url`, `ObjectData.media_url` (the legacy
    object-media URL), `LoadUrlRequest.url`, the `seed_capability` on
    `Event::NeighborSeed` and `LoginSuccess` plus the CAPS `TeleportFinish` /
    `CrossedRegion`, and the user-supplied
    `LoginParams.login_uri`/`LoginHttpRequest.url`. The seed
    plumbing is typed end-to-end (`Session`/`Client` `seed_capability()` →
    `Option<&url::Url>`, the `child_seeds` map, the bevy `SlIdentity`). Decode
    stance: LLSD/caps + the UDP `parcel_info`/`object_from_full_update`
    (the latter made fallible) propagate `WireError::InvalidUrl`; the
    `Option`-returning CAPS/compressed decoders drop the record on a bad URL
    (`.ok()?`, matching their existing None-drop philosophy); the **inline
    teleport seeds** (`TeleportFinish`/`CrossedRegion`) are a hard error on a
    non-empty unparsable value (per user follow-up) while an empty value still
    falls back to the cached child seed. **Left raw `String`** (per the user's
    decision: user-entered free text the viewer does not interpret):
    `AvatarProfile.profile_url`/`ProfileUpdate.profile_url`; also the
    `MediaEntry.whitelist` patterns (glob wildcards, not parseable URLs). REPL
    gains an `opt_url` arg helper; both runtimes' caps/login plumbing pass
    `url::Url`; survey's `--login-uri` clap arg parses straight to `url::Url`.
    +4 `url.rs` unit tests (empty→None, valid round-trip, SLURL scheme,
    rejection); lifecycle + `sim_session` suites updated. NO sl-types touched.
    **Phase 8 COMPLETE.**
