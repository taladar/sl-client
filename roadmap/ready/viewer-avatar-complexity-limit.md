---
id: viewer-avatar-complexity-limit
title: Avatar complexity limiting (jellydoll)
topic: viewer
status: ready
origin: render-feature gap analysis vs Firestorm (2026-07); split from viewer-avatar-impostors
refs: [viewer-quick-preferences]
---

Context: [context/viewer.md](../context/viewer.md).

Cap over-heavy avatars so a single griefer-built avatar cannot sink the frame
rate. Score each avatar's render cost (triangles, textures, attachments) and,
past a budget, draw it as a flat "jellydoll" silhouette rather than its real
(attachment-heavy) geometry.

Firestorm drives this from `RenderAvatarMaxComplexity` (the budget) and
`RenderAvatarComplexityMode` (how the cap is applied). Needs a complexity metric
per avatar and the fallback jellydoll render.

Scope: the complexity score, the jellydoll fallback render, the budget
threshold, and the user controls — including a per-avatar "always render fully /
never" override. Relates to the R22 avatar-render work and pairs with the
impostor selection ([[viewer-avatar-impostors-billboard]]).

Quick Preferences: `RenderAvatarMaxComplexity` is a reached-for-hourly knob, so
when the budget setting lands add a default entry for it in the Quick
Preferences panel ([[viewer-quick-preferences]]) — a line in `default_entries()`
(`quick_preferences.rs`) plus a Fluent label. The panel binds by setting key, so
the entry needs only the key, range and label (it was left out of the panel's
first version precisely because the setting did not exist yet).

Reference (Firestorm, read-only): the `llvoavatar` complexity path,
`RenderAvatarMaxComplexity` / `RenderAvatarComplexityMode`.

Builds on: the avatar rendering (P12–P18) and the coarse / interest distance
tracking already in `avatars.rs`.
