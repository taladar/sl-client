---
id: test-firestorm-harness-skin-selection
title: Firestorm harness — start a run in a named skin and theme
topic: test
status: ready
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
`SkinCurrent`). It is not sufficient for three separate reasons:

- **It is persistent.** `SkinCurrent` is `Persist=1`, so `map-to` writes it
  into the settings file on exit. That is the whole class of bug
  `FSTestHarness::forceSetting` exists to avoid: it sets through
  `control->setValue(value, false)`, the non-persisting path.
- **There is no theme option at all.** `SkinCurrentTheme` has no command-line
  spelling, and a skin without its theme is a different skin — `firestorm`
  alone is ambiguous between Grey, Dark, Blue, High Contrast and CtrlAltStudio.
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

- `--skin <folder>` and `--theme <folder>` on the harness, plus
  `SL_VIEWER_SKIN` / `SL_VIEWER_THEME` — the same names sl-client's viewer
  already uses, so one environment block keeps dressing both viewers.
- Force all four settings through `forceSetting`, so a run leaves the user
  directory's skin choice alone even when it is not a throwaway directory.
- Resolve the readable names from `skins.xml` (`getSkinBaseDir()`, the file
  the prefs panel reads at `llfloaterpreference.cpp:5218`) rather than
  asking the caller for them twice. Fail the run with the list of available
  skins when a folder does not resolve — a typo must not silently produce a
  default-skin run, which is the failure this whole task is about.
- **Mind the empty theme folder.** Vintage's only theme, `Classic`, has an
  empty folder string, and so does each skin's first theme. "No theme given"
  and "the theme whose folder is empty" must not be the same value internally,
  or `--skin vintage` cannot be distinguished from a caller who meant the
  base.
- Skip the prefs panel's side effects. `LLPanelPreferenceSkins::apply()` also
  clobbers toolbar settings (`FSSkinClobbersToolbarPrefs` →
  `ResetToolbarSettings`), nudges nav-bar settings for the starlight skins,
  and raises a restart notification. A harness run wants none of those — it
  is choosing the skin *before* the UI exists, not switching one at runtime.

## On the sl-client side

`sl-crosscheck` grows one `--skin` / `--theme` pair that goes into the shared
capture block and reaches both viewers, exactly as `--fov` does. sl-client's
own binary already takes `--skin` / `--theme`, so its half is wiring only.

## Done when

`--skin vintage --theme Classic` brings the harness up wearing Vintage — art
*and* the legacy code paths (`FSInternalSkinCurrent` reads `Vintage`) — the
run directory's settings are unchanged afterwards, an unknown skin fails the
run naming the available ones, and `sl-crosscheck --skin …` dresses both
viewers from one flag.
