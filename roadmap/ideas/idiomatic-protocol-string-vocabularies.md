---
id: idiomatic-protocol-string-vocabularies
title: Closed protocol vocabularies kept as strings
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns,
  protocol-login-options-max-agent-groups-unrequested]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What

Fixed vocabularies passed as strings in the protocol crates (all match today):
EstateOwnerMessage / GenericMessage method names (~20 literal sends in
`methods.rs`, constants in `sim_session.rs`, a third copy in sl-fake-grid,
`ServerEvent::EstateOwnerRequest { method: String }`,
`Command::SendGodlikeMessage { method: String }`) and the telehub / terrain
sub-commands; event-queue message names (`enqueue_caps_event("TeleportFinish",
…)` ~20 calls although `CapsEvent` exists with a crate-private `from_tag`);
the settings asset tag `"sky"` / `"water"` / `"daycycle"` (trap:
`SettingsKind::as_str()` says `"day"`); voice `voice_server_type` /
`channel_type` / `jsep_type: Option<String>`; maturity short codes
(`max_access_pref`, login `agent_access*`) beside `Maturity`'s parsers;
`ChatSessionRequest` methods as `&str`; the upload response state machine as
`state: String` plus `Option`s; script compile target as a string beside
`ScriptTarget`; login `options` as free strings. Bespoke-to-this-workspace:
the grid nickname table copied three times (`sl-repl-*`, viewer).

This is about these vocabularies, not the deferred `CAP_*` capability names.

## How

`EstateMethod`, `TelehubCommand`, `TerrainCommand`; public `CapsEvent` with
`as_tag()` taken by the enqueue; `SettingsKind::asset_tag()`;
`VoiceServerType` / `VoiceChannelType` / `JsepType`; `Maturity` with serde
short codes; `ChatSessionMethod`; `enum UploadResponse { Upload { uploader },
Complete { … }, Failed { … } }`; `LoginOption`; a shared `KnownGrid`.
