---
id: viewer-avatar-impostors-billboard
title: Billboard impostors for distant avatars
topic: viewer
status: deferred
origin: render-feature gap analysis vs Firestorm (2026-07); split from viewer-avatar-impostors
refs: [viewer-quick-preferences, viewer-perf-gpu-avatar-crowd]
---

Context: [context/viewer.md](../context/viewer.md).

> **Reframed (2026-08-12): optional low-end / extreme-count fallback,
> deferred.** Impostors are the reference viewer's *workaround* for a slow
> full-geometry avatar path — they trade fidelity (a flat, occasionally-
> refreshed billboard) for speed. On capable hardware the full path already
> does ~30 avatars fine, and the primary crowd strategy here is to make the
> *real* path scale ([[viewer-perf-gpu-avatar-crowd]]: compute-pass GPU
> animation + same-body instancing), which is full-quality and benefits every
> tier — pushing the "need impostors" threshold far out. So impostors are **no
> longer a near-term must**: implement them only as an **opt-in fallback**
> (behind a quality preference, **default off** on capable hardware) for (a)
> genuinely low-end GPUs and (b) extreme mega-event counts where even instanced
> real geometry saturates. We copy the reference's *goal* (survive a crowd),
> not this *mechanism*. The design below still stands for when/if it is built.

The performance feature that makes a crowded region survivable: past a limit,
distant avatars are rendered as flat **billboard impostors** — a cached snapshot
re-rendered occasionally — instead of full geometry. In a busy club the
difference between this and its absence is tens of frames per second.

Beyond `RenderAvatarMaxNonImpostors` (surfaced as `IndirectMaxNonImpostors`),
the *N* nearest avatars render fully and the rest become impostors: render each
to its own small target, draw that as a camera-facing billboard, and refresh it
only when the avatar moves / animates or the view angle changes enough. This is
the same render-to-texture idea as the P33 probes, applied per distant avatar.

Scope: the nearest-N selection (we already track avatar distances), the impostor
render target + billboard + refresh policy, and the user control for the limit.
Relates to the R22 avatar-render work.

Quick Preferences: `RenderAvatarMaxNonImpostors` (`IndirectMaxNonImpostors`) is
a reached-for-hourly knob, so when the limit setting lands add a default entry
for it in the Quick Preferences panel ([[viewer-quick-preferences]]) — a line in
`default_entries()` (`quick_preferences.rs`) plus a Fluent label. The panel
binds by setting key, so the entry needs only the key, range and label (it was
left out of the panel's first version precisely because the setting did not
exist yet).

Reference (Firestorm, read-only): the `llvoavatar` impostor path,
`RenderAvatarMaxNonImpostors`.

Builds on: the avatar rendering (P12–P18) and the coarse / interest distance
tracking already in `avatars.rs`.
