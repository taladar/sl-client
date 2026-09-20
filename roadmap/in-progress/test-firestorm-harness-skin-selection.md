---
id: test-firestorm-harness-skin-selection
title: Firestorm harness — start a run in a named skin and theme
topic: test
status: in-progress
origin: Vintage skin fidelity audit (2026-09-20)
points: 3
refs: [test-firestorm-crosscheck-runner, test-firestorm-fake-grid-crosscheck,
       viewer-vintage-ui-chrome-crosscheck, viewer-vintage-skin]
---

Context: [context/testing.md](../context/testing.md),
[context/vintage-skin.md](../context/vintage-skin.md).

A cross-check run hands Firestorm a fresh `FIRESTORM_X64_USER_DIR` — on
purpose, so it never touches the operator's settings — which means it comes up
in the **default skin** every time. Any comparison of our skin against "the
reference's Vintage" is therefore currently a comparison against Firestorm's
`firestorm/grey`, and says nothing.

The fix belongs in the fork's harness (branch `test-harness`), beside
`--credentials` / `--gridfile` / `--camera-position`, not in settings files
`sl-crosscheck` writes into the run directory: the harness already owns the
"force a setting for this run only" idiom, and a skin is exactly that kind of
setting.

## What already exists, and why it is not enough

Upstream has a `--skin` option (`app_settings/cmd_line.xml`, `map-to`
`SkinCurrent`), and it does one thing right that is worth stating so nobody
"fixes" it: it does **not** persist. `map-to` applies through
`ctrl->setValue(value, false)` (`llcommandlineparser.cpp:621`), the same
non-persisting path `FSTestHarness::forceSetting` uses, so a run cannot leave
the operator's viewer repainted. Keep it; build beside it.

It is still not sufficient, for three reasons:

- **There is no theme option at all.** `SkinCurrentTheme` has no command-line
  spelling, and a skin without its theme is a different skin — `firestorm`
  alone is ambiguous between Grey, Dark, Blue, High Contrast and CtrlAltStudio.
- **Nothing validates the name.** `setSkinFolder` only appends the folder to
  the search path, so a typo or the wrong case silently resolves every lookup
  to the default skin — a run that captures the wrong skin and says nothing.
- **It sets the folder, not the identity.** Firestorm keeps *three* pairs of
  skin settings, and this option reaches only the first.

| Pair | Persist | Who reads it |
| --- | --- | --- |
| `SkinCurrent` / `…Theme` | 1 | `setSkinFolder` — where files load from |
| `FSSkinCurrentReadableName` / `…ThemeReadableName` | 1 | the prefs panel |
| `FSInternalSkinCurrent` / `…Theme` | 0 | **behaviour checks** |

`llstartup.cpp:790` copies the readable names into the internal pair at
startup, and the internal pair is then read as a *behaviour* switch, not a
label: `fscommon.cpp:495` (`is_legacy_skin = FSInternalSkinCurrent ==
"Vintage"`), `llviewermenu.cpp:5436`, `fsfloaterim.cpp:422`. Set only the
folder and the run renders Vintage's art while taking the modern skin's code
paths — a divergence that would look like our bug.

## Where it goes

`FSTestHarness::initFromCommandLine()` is called from
`LLAppViewer::initConfiguration()` at `llappviewer.cpp:3280`; the skin block
that reads `SkinCurrent`, calls `gDirUtilp->setSkinFolder()` and then
`loadSettingsFromDirectory("CurrentSkin")` is at `:3413` — **~130 lines
later, in the same function**. So the harness's existing hook point is already
early enough, and no new hook is needed. The readable-name pair is copied to
the internal pair later still (`llstartup.cpp:790`), so forcing all four in
`initFromCommandLine` is in time for every consumer.

## What to add

- A `--theme` option beside upstream's `--skin`, plus `SL_VIEWER_SKIN` /
  `SL_VIEWER_THEME` — the same names sl-client's viewer already uses, so one
  environment block keeps dressing both viewers.
- Force all four settings through `forceSetting`, so a run leaves the user
  directory's skin choice alone even when it is not a throwaway directory.
- Resolve the readable names from `skins.xml` (`getSkinBaseDir()`, the file
  the prefs panel reads at `llfloaterpreference.cpp:5218`) rather than
  asking the caller for them twice. Fail the run with the list of available
  skins when a name does not resolve — a typo must not silently produce a
  default-skin run, which is the failure this whole task is about.
- **Accept either spelling**, case-insensitively: the folder (`vintage`) or
  the name the preferences panel shows (`Vintage`). For a theme this is not a
  convenience but the only workable interface — **a skin's first theme has an
  empty folder**, so Vintage's sole theme is askable for as `Classic` and
  spellable as nothing at all.
- Keep "no theme given" distinct from "the theme whose folder is empty". A
  skin named *without* a theme must take its own first one; keeping the
  outgoing skin's theme folder would point the search at a
  `skins/<new>/themes/<old>` that does not exist.
- Reconcile, do not only set: an armed run with no skin named should still
  make the three pairs agree with whatever `SkinCurrent` holds, so a user
  directory whose pairs have drifted cannot quietly describe a run as
  something it is not.
- **A command-line `--skin` *can* be told from a saved choice**, which is
  what makes it work on its own rather than only alongside `--theme`. A
  control's values are a stack — default, saved, then unsaved overrides — and
  `map-to` pushes an unsaved value, which by construction cannot reach
  `getSaveValue()` (`llcontrol.cpp:236`, and it says so). So
  `getValue() != getSaveValue()` on `SkinCurrent` is exactly "something
  overrode this for this run".
- **Naming a skin must not arm the harness.** Which skin the viewer wears is
  a presentation choice, not a run mode; arming on it would turn an ordinary
  interactive session into an unattended one — closing its floaters and
  forcing the determinism settings on it. So the skin is applied on both sides
  of the `!mActive` early return, unlike every capture option.
- Skip the prefs panel's side effects. `LLPanelPreferenceSkins::apply()` also
  clobbers toolbar settings (`FSSkinClobbersToolbarPrefs` →
  `ResetToolbarSettings`), nudges nav-bar settings for the starlight skins,
  and raises a restart notification. A harness run wants none of those — it
  is choosing the skin *before* the UI exists, not switching one at runtime.

## On the sl-client side

`sl-crosscheck` grows **per-viewer** skin options —
`--sl-client-skin` / `--sl-client-theme` and
`--firestorm-skin` / `--firestorm-theme` — and deliberately **no** shared
`--skin`.

The first cut of this got it wrong in a way worth recording. The obvious move
is to put the skin in `CaptureSpec`, the block that reaches both viewers by one
environment block, beside `--fov` and the day position. But that block exists
precisely because those settings must be *identical* on both sides, and a skin
is the one setting that cannot be: the namespaces are unrelated (`graphite`
here, `vintage`/`Vintage` there) and there is no reason to expect them to
converge. Themes are further apart still and permanently so — ours is an
overlay id naming a file under `themes/`, the reference's is a folder *or* a
display name, and its first theme of every skin has an **empty folder**, a wart
we are not reproducing. So there is no spelling of "the base theme" the two
share.

For the same reason the two are **different types** (`SlClientSkin` and
`FirestormSkin`), not one type used twice: a value valid for one is generally
invalid for the other, and one shared type would let a run hand a viewer the
other's skin — surfacing as a capture of the wrong interface rather than as an
error. Only the two environment variable *names* are shared, which is the one
thing that genuinely is.

Each launcher extends its own environment with its own viewer's skin;
`CaptureSpec` keeps only what really is common. `run.json` records both sides,
because a `ui` capture cannot be read without knowing which skin each viewer
wore. And because our side silently degrades where Firestorm's harness now
refuses — an unknown id reaches `bevy_flair` as a stylesheet that fails to
load, capturing an *unstyled* interface — the runner checks our asset tree
ships the named skin before launching anything.

## Landed — the Firestorm half (2026-09-20)

`FSTestHarness::applySkin()` on the fork's `test-harness` branch, plus a
`theme` entry in `cmd_line.xml` and an `FSTestSkinTheme` setting. Upstream's
`--skin` is untouched; everything above is built beside it.

Verified against the packaged build, one run each:

| Route | Result |
| --- | --- |
| `--skin vintage` | `skin: Vintage / Classic`, loads `skins/vintage/settings.xml` |
| `SL_VIEWER_SKIN=Vintage` | same — the name resolves to the folder |
| `SL_VIEWER_SKIN=firestorm SL_VIEWER_THEME=Dark` | folders `firestorm` / `dark` |
| unknown skin / unknown theme | refuses, listing the choices |
| no skin option at all | no harness log line, default skin |

No run left `SkinCurrent` in the run directory's `settings.xml`.

One link in the chain is read rather than observed: nothing logs
`FSInternalSkinCurrent`, so that the forced readable name reaches the
behaviour switch rests on `llstartup.cpp:790` copying it unconditionally at
`STATE_FIRST`. Checked that nothing else writes those keys before then — no
skin's own `settings.xml` mentions them, and every other writer is in the
preferences floater, which an unattended run never opens.

Still open: the `sl-crosscheck` half below.

## Done when

`--skin vintage --theme Classic` brings the harness up wearing Vintage — art
*and* the legacy code paths (`FSInternalSkinCurrent` reads `Vintage`) — the
run directory's settings are unchanged afterwards, an unknown skin fails the
run naming the available ones, and `sl-crosscheck --firestorm-skin vintage
--sl-client-skin <ours>` dresses each viewer in a skin of its own.
