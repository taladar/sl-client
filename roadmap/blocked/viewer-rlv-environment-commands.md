---
id: viewer-rlv-environment-commands
title: RLV — @setenv_*/@getenv_* environment control
topic: viewer
status: blocked
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-environment-fixed-editor, viewer-environment-personal-lighting]
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

RLV lets a worn object drive the wearer's local environment:
`@setenv_<subkey>:<value>=force` sets roughly forty sky/water/day
parameters and `@getenv_<subkey>=<channel>` reads them back
(`rlvenvironment.cpp`). Subkeys include daytime, preset/asset/daycycle by
name or UUID, ambient, bluedensity, bluehorizon, densitymultiplier,
distancemultiplier, dropletradius, hazedensity, hazehorizon, icelevel,
maxaltitude, moisturelevel, scenegamma, cloudcolor, cloudcoverage,
clouddensity (plus legacy "cloud"), clouddetail, cloudscale, cloudscroll,
cloudtexture, cloudvariance, moonbrightness, moonscale, moontexture,
sunglowsize, sunglowfocus, sunlightcolor (plus legacy "sunmooncolor"),
sunscale, suntexture, starbrightness, sunazimuth, sunelevation,
moonazimuth, moonelevation, eastangle and sunmoonposition, with legacy
per-component r/g/b and x/y suffixes handled via `idxComponent` in
`RlvEnvironment::onHandleCommand`. The `@setenv=n` gate (a dictionary
restriction our parser already knows) forbids the user opening the
environment editors while scripts control the sky, and the
RestrainedLoveNoSetEnv setting opts out entirely.

These are extension-prefix commands outside the behaviour dictionary —
Firestorm dispatches them through the `RlvExtCommandHandler` fallback, and
our parser today faithfully yields `RlvBehaviour::Unknown` with the raw
keyword kept (`sl-rlv/src/behaviour.rs`). Implementing this means
recognising the `setenv_`/`getenv_` prefixes on Unknown keywords, mapping
each subkey onto our EEP-based environment override layer — the
[[viewer-environment-personal-lighting]] local override is the natural
write target, with [[viewer-environment-fixed-editor]] providing the
editors the `@setenv=n` gate must lock — and answering `@getenv_*` on the
requested chat channel.

Reference (Firestorm, read-only): `indra/newview/rlvenvironment.cpp`,
`indra/newview/rlvenvironment.h`.
