---
id: viewer-name-tags-complexity-distance
title: Name tags — complexity (ARC) lines
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22); nametag feature survey
blocked_by: [viewer-avatar-complexity-limit]
refs: [viewer-name-tags-decorations, viewer-name-tags-billboard-render]
---

Context: [context/viewer.md](../context/viewer.md).

The Firestorm complexity tag additions (the survey's `FSTag*` family), on
top of the tag renderer and the complexity computation:

- **Complexity (ARC) line** — the avatar's render cost in the tag, with
  the reference's three modes: own tag only (`FSTagShowOwnARW`), every
  avatar (`FSTagShowARW`), or only too-complex/jellied avatars
  (`FSTagShowTooComplexOnlyARW`); coloured green→red against the
  complexity limit, plus the red texture-area line when attachment
  surface area is the jelly reason.

The **distance line and range colouring** originally scoped here shipped
with [[viewer-name-tags-billboard-render]] (2026-08-05): "N.NN m" measured
from the **own avatar** (the camera-based distances govern only the
fade/cut-off — a deliberate split), tinted by the whisper / say / shout /
beyond bands, with the whole-tag range tint behind the `ColorByDistance`
setting (default off, like the reference).

Reference (Firestorm, read-only): `llvoavatar::idleUpdateNameTagText`
(`FSTagShow*`), `llhudnametag`.

Deps: [[viewer-avatar-complexity-limit]] (the ARC numbers + jelly
reasons); the tag surface ([[viewer-name-tags-billboard-render]]) is done.

## Built

The two render-cost lines, in the tag composer
([`name_tag_content.rs`](../../sl-client-bevy-viewer/src/name_tag_content.rs)),
over the scores [[viewer-avatar-complexity-limit]] measures:

- **`Complexity: N`** on a green→amber→red ramp of the cost against the
  complexity budget — the reference's arithmetic unchanged, so green at
  nothing, amber at exactly the budget, saturating red at twice it. With **no
  budget set there is nothing to judge against**, so the number is reported in
  neutral grey (the reference's `grey1`) instead of rated.
- **`Texture Area: N m²`** in red, only when an avatar's attachments cover more
  than the area limit. It appears *alongside* the cost rather than instead of
  it: the reference notes that untangling which limit actually fired would cost
  more than it explains, and shows the cost either way.

Both sit after the distance line, at the small font tier, and both are pure
functions of the composer's inputs — `shows_complexity` and `complexity_color`
are unit-testable on their own.

**The three settings are the reference's, defaults included, and they compose
to a quiet default.** `ShowComplexity` (on) is the master switch;
`ShowComplexityWhenLimitedOnly` (on) means other avatars show the number only
while the limiter is actually doing something about them — so out of the box a
crowd grows no new text at all, and a jellydoll explains itself the moment it
appears. `ShowOwnComplexity` (off) is the one you turn on to find out whether
*you* are the expensive one: the radar lists nearby avatars and excludes you,
exactly as the reference radar does, so this line is your only read-out of your
own ARC. All three are bound in Preferences ▸ General ▸ Name tags beside the
other tag toggles.

**Your own cost is reported, never rated.** The reference skips the ramp for
`isSelf()` and so do we: the limit does not apply to you, so colouring your own
tag red would be telling you off for nothing.

## Divergences

- **An unscored avatar shows no line at all**, rather than a zero. The
  reference has no such state — it computes complexity synchronously — while
  ours is measured on a debounced budget, so "not measured yet" is a real
  situation and must not read as "costs nothing".
- **The line text is English**, like every other composed tag line (`Away`,
  `Blocked`, `Typing`): `compose_tag` is a pure function with no translator, so
  localising these means localising the tag composer as a whole — a separate
  change, and the same gap the status lines already have.
- **The reference's `grey1` and red are hardcoded here too.** Unlike the
  distance bands they are not `colors.xml` entries in the reference, so they
  are not skin tokens.
- **"Only when limited" keys off whether the viewer is drawing the avatar as a
  jellydoll**, which includes an avatar you pinned to *Never render*. The
  reference's `isTooComplex()` excludes that case, so it would hide the number
  for an avatar you personally chose not to draw — ours explains itself there
  too, which is the more useful answer.

## The own avatar's cost had to be made real first

The feature exists so you can see *your own* number — the radar lists other
avatars — and on first live run it read `Complexity: 0` while the body plainly
rendered. Two causes, both fixed here, and worth knowing because neither is
visible from the code alone:

- **Your own bakes are not in `baked_textures`.** The cost charges per
  *published* baked region, read from the `AvatarAppearance` each avatar sends.
  Your own avatar never receives its own back, and on a client-side-baking grid
  it composites its regions locally instead (P15.3) — the own body is draped
  straight from those composited images, so `bake_publish` never mirrors the
  published ids into `baked_textures` either. The local composite's region count
  now stands in for the own avatar, which is the same split the reference makes
  (`isIndexLocalTexture` / `isTextureDefined(index, 0)` for self).
- **The composite finishes *after* the first score.** It is a background job; on
  the live run the avatar was scored at `…:17` and the composite landed at
  `…:22`. Nothing in the staleness pass would have noticed — no object changed,
  no appearance arrived — so the score was measured once, at zero, and stayed
  there for the session. The pass now watches the composite's region count and
  re-scores the own avatar when it moves.

## Verification

Unit-tested: the three-setting visibility rule in every combination (master
switch off wins over both, own tag needs its own opt-in, others always vs only
when limited); the colour ramp at nothing / at the budget / at twice it /
saturating past it, and the unrated grey when no budget is set; the cost line's
position after the distance line, its text and its ramp colour; an unscored
avatar producing no line; the texture-area line appearing only above the limit,
in red, and never when the area limit is off; and the own tag reporting its
cost unrated.

Live-verified against the local OpenSim: with **Show own complexity** ticked the
own tag reads `Complexity: 1000` in neutral grey — five composited baked regions
at 200 apiece, exactly the arithmetic — and the log shows the sequence the fix
targets (`body=0` measured at `20:35:17`, composite at `20:35:22`, re-score
`body=1000` 0.2 s later). The other avatar reads 0, correctly: the `sl-repl`
second avatar publishes no bakes (which is why it renders untextured) and wears
no attachments, so it genuinely costs nothing to draw. The visibility rule was
exercised across all three toggles.

Not verifiable on this grid: the green→amber→red ramp, since every score here
is either 0 or the un-limited own avatar's, both of which render in the unrated
grey. The ramp arithmetic is unit-tested; seeing it wants aditi, alongside the
score magnitudes already noted for [[viewer-avatar-complexity-limit]].
