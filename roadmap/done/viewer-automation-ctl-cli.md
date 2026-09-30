---
id: viewer-automation-ctl-cli
title: sl-viewer-ctl — drive a running viewer from the shell
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 5
blocked_by: [viewer-automation-driver]
refs: [test-e2e-stage, viewer-automation-mcp-server]
---

Context: [context/automation.md](../context/automation.md).

The same driver, interactively: for a developer poking at a live viewer,
and for an agent that would otherwise ask the user to "log in and check".

## Wanted

- `sl-viewer-ctl launch` (a viewer with automation, headless or `--watch`,
  against a grid), `stage` (a fake grid plus viewers from a small TOML),
  and `attach <socket>`.
- Verbs: `tree [<selector>]`, `find`, `click`, `fill`, `press`, `wait`,
  `world find`, `world touch`, `chat`, `notifications`, `agent`,
  `screenshot <png>`, `events --follow`.
- The **string selector grammar** lands here in `sl-automation-proto`
  (parse and print, round-trip tested), e.g.
  `window[test_id=build] >> button[name_key=build-apply]`, and failure
  messages print locators in it.
- Human output by default, `--json` for scripts.

Acceptance: from a shell, launch a headless viewer on the fake grid, open
the Build floater by locator, read a disabled button's state and save a
screenshot; the grammar round-trips every locator shape.

## Done (2026-09-30)

- **The selector grammar** (`sl-automation-proto/src/selector.rs`):
  `Locator` and `WorldLocator` implement `FromStr` and print (`Display`) in
  one grammar — `window[test_id=floater:build] >>
  button[name_key=build-apply][enabled=false]`,
  `object[name=Door][near=own_avatar][radius=5][nth=0]`. Attributes are the
  JSON field names; `~=` is a name's "contains"; values are bare when plain
  and quoted (with escapes) otherwise. A mistake is a `SelectorError`
  naming the column. The old display-only form is gone, so every failure
  message now prints a pasteable selector (`Role::ALL`, `WorldKind::ALL`
  back the parser). Round-trip tests cover every field, each role and
  kind, awkward strings (quotes, controls, `]`, `>>`, non-ASCII, empty),
  both anchors, and nested scopes.
- **`sl-viewer-ctl`** (new crate; no Bevy, no viewer library — the fake
  grid, the driver, `sl-viewer-launch`, `sl-repl`'s credentials and
  cooldown):
  - `launch --credentials F [--avatar K] [--grid G | --login-uri U]
    [--start S] [--watch] [--web-media] [-- viewer args]`: the release
    viewer beside the CLI, `--headless`, confined to a run directory under
    `$XDG_STATE_HOME/sl-viewer-ctl/runs/`, its socket in the viewer's
    default socket directory; waits until it arrived and went quiet, prints
    its socket, log and an `export SL_VIEWER_SOCKET=…` line, holds it until
    Ctrl-C / `SIGTERM`, then logs it out. An aditi login takes its turn
    under the shared `LoginCooldown`.
  - `stage <toml>`: a fresh fake grid (`scenario`, `port`) and each
    `[[viewer]]` (`label`, `watch`, `web_media`, `args`) as account
    `Stage <label>`, started in parallel; a viewer that fails to come up
    takes the others down with it.
  - `attach [socket]`: one connection, commands from stdin one per line
    (shell-split, `#` comments, `quit`); a failed line is printed and
    counted (non-zero exit), a closed connection ends the session.
  - Verbs: `tree [sel] [--depth N]`, `find`, `click [--right|--double]`,
    `fill`, `press [--on sel]`, `wait --for
    attached|detached|visible|hidden|enabled|disabled|text=…|text~=…`,
    `open <floater>`, `menu <key>…`, `world find|touch [--no-reveal]`,
    `chat` (the transcripts), `notifications`, `agent`, `screenshot <png>
    [--outline sel]`, `events [--stream …] [--from N] [--follow]`. Without
    `--socket` / `SL_VIEWER_SOCKET` a verb uses the one socket that accepts
    a connection in the default directory, and lists them when there are
    several. Selectors may also be given as the locator's JSON.
  - Text output by default, `--json` one document per result (a JSON line
    per entry under `events --follow`); `--artifacts DIR` saves a
    failure's screenshot, tree and event tail; `--timeout` per command.
- **Driver**: `Viewer::subscribe_from(cursor, streams)` — the protocol's
  subscription already took a cursor — so `events --follow --from N` has
  no gap between what was kept and what comes next.
- **Tests**: grammar round-trips and refusals (`selector/tests.rs`); the
  CLI against a scripted viewer (`sl-viewer-ctl/src/tests.rs`: what each
  verb asks, text and JSON output, depth cut, a failure printed in the
  grammar, an `attach` script with a bad line and `quit`); socket
  discovery ignores stale sockets; stage-file validation.
- **Acceptance, from a shell** (release build): `stage` with one viewer on
  the fake grid was ready ~3 s after start; with no `--socket`, `click
  'button[name_key=bottom-toolbar-build]'` opened the Build window,
  `--json wait 'window[test_id=floater:build-tools] >>
  button[name_key=build-content-new-script]' --for disabled` read its
  disabled state (and `--for enabled` failed with exit 1 and the locator
  printed in the grammar), `screenshot build.png --outline
  'window[test_id=floater:build-tools]'` saved the frame with the window
  outlined; `attach` ran `agent`, `world find`, `chat`, `notifications`,
  `events`; `world touch` landed; `events --follow` streamed until
  `SIGINT`; `SIGINT` to the stage logged the viewer out cleanly and removed
  its socket. `launch` did the same on the local OpenSim (a `SIGTERM`
  logout) and on aditi.
- Seen on the way, not changed: at 1280×720 the Build window (80 px from
  the top, 671 px tall) runs past the frame's bottom edge.
