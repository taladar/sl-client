# Driving the viewer: automation and end-to-end tests

The viewer can be driven like a browser under Playwright: a test (or a
person at a shell, or an agent) finds UI elements and in-world objects by
what they *are*, plays real input at them, and reads back text, state and
models — with no human in the loop and no window on the desktop. This
chapter is the map of that tier; the design notes and their history live in
the roadmap's `context/automation.md`.

Every lower test tier cuts the app somewhere (see
[The viewer test harness](test-harness.md)). This one runs the **whole**
viewer — the real binary, or the same App built in the test process — so it
is the tier for what only the whole app, or several of them, can break.

## The pieces

| Crate | What it is |
| --- | --- |
| `sl-automation-proto` | The vocabulary: locators, requests, responses, probes, the selector grammar. Pure serde, no Bevy. |
| `sl-viewer-automation` | The in-viewer half: the semantic model, the locator engine, the executor, the transports. |
| `sl-viewer-driver` | The async client a test or a tool holds: one `Viewer` per viewer, over either transport. |
| `sl-viewer-launch` | Starting and stopping viewer processes, each confined to its own directory. |
| `sl-e2e` | The `Stage`: a fake grid plus N logged-in viewers per test, and the teardown. |
| `sl-viewer-ctl` | The driver from a shell. |

```text
 test (cargo/nextest)          sl-viewer-ctl (shell / agent)
        |                               |
   sl-e2e Stage --------------- sl-viewer-driver
   | fake grid (in process)        |
   | N viewers --------------------+- in-process transport -> ViewerApp
   |                               +- remote transport (JSON lines, Unix socket)
   |                                           v
   +------------------------------> viewer process --headless --automation-socket
```

## The semantic model

The automation never addresses a pixel. It reads a **semantic tree** built
on request from the UI's entities — the same tree the viewer hands AccessKit
for screen readers. Each node has:

- a **role** (`button`, `checkbox`, `textbox`, `combobox`, `slider`,
  `spinbutton`, `colorwell`, `tab`, `menuitem`, `listitem`, `treeitem`,
  `window`, `text`, `image`, `document`, …), from a widget's `Semantic`
  component or inferred from the stock Bevy widgets;
- a **name** as the user reads it, and its **Fluent key** where it came
  from a translation — the locale-independent way to address a node;
- a **test id**: the entity's `Name` (`floater:build`,
  `build-pos-x:field`);
- **states** (`disabled` — inherited from any ancestor, `checked`, `selected`,
  `expanded`, `focused`, `hovered`, `read_only`), a **value** (a field's text, a
  combo's choice, a slider's or a spin button's number, a colour well's
  `#rrggbb`), a text node's **colour**, its **bounds** in logical pixels and its
  **visibility** (`visible`, `hidden`, `clipped`, `off_screen`, `covered`).

A widget the model cannot infer says what it is with a `Semantic` on its
root at spawn (`sl_viewer_ui_core::semantic`). A list row is named by its
cells, not by a button inside it; a browser view is a `document` named by
its page's title and valued by its address. A numeric field with step
arrows (`ui_spinner`) is a `spinbutton`: `fill` types into it like a
`textbox`, and its two arrows are buttons beside it in the spinner's
group (`{element}:spinner`), `{element}:up` and `{element}:down`, called
"Increase" and "Decrease".

## Locators and selectors

A locator names nodes by role and attributes, scoped by `within`; a world
locator names avatars, objects or attachments by name, owner, id or nearness.
Both have a string grammar, and a failure prints the locator in it so it can
be pasted back:

```text
window[test_id=floater:build] >> button[name_key=build-apply][enabled=false]
window[test_id=floater:inventory] >> treeitem[name="Stage Notecard"]
listitem[name~=Seated][nth=0]
object[name=Door][near=own_avatar][radius=5]
```

`~=` is "contains"; a value is bare when it is plain and quoted otherwise.

**Locators are strict**: an action on a locator that matches two nodes is an
error listing both, never "the first one". Name a row (`[name~=…]`) or pick
one (`[nth=0]`).

## Actions, waits and probes

Requests go into the viewer's `AutomationQueue` and are answered from it;
nothing runs while none are queued.

- **UI actions** — `click` (left, right, double), `hover`, `fill`, `press`
  (a key or chord; `hold_frames` keeps it down), `drag_to`, `drag_by`,
  `select_option`, `menu_path`, `open_floater`. Before every action the
  target must be attached, visible, *stable* (unchanged for a few frames),
  enabled and the one a hit test at its aim point lands on; a scroll area or
  a virtual list is scrolled to bring it into view. Input goes through the
  real input path (`sl_viewer_ui_core::synthetic_input`), so a test that
  says "a disabled button does nothing" can fail.
- **World actions** — `touch`, `double_click`, `open_pie`, `hover`,
  `select`, `shift_select`, `place` (rez with the Create tool), `drop_from`
  (a drag from a UI node). An aim point is a projected point the viewer's own
  pick resolver confirms lands on the target; a target nothing reaches is
  framed by the camera once ("reveal") and looked at again until the
  deadline.
- **Ground actions** — the same gestures (but no select) on a point of the
  ground: `viewer.world().ground("Next Door", 20.0, 128.0).double_click()`.
  Bare ground has no id, so it is addressed by a region's name and a
  position in it; the height is the terrain's. The pick must find the ground
  at that point, so an object, an avatar or water standing on it, or a UI
  node over it, makes it not actionable. The answer is where the click
  landed, in that region's metres.
- **Waits** run in the viewer, a predicate over the model each frame with a
  frame and a wall deadline: a node attached, detached, visible, hidden,
  enabled, disabled, checked, holding a text; a world thing present or gone;
  a probe's readout holding a value at a JSON pointer. A test never sleeps.
- **Probes** read the viewer's models, not its widgets: `agent` (region,
  position, seat, teleport, camera mode and eye, heading), `status`,
  `conversations`, `notifications`, `selection`, `inventory` (one folder by
  name path), `inventory_tree` (every folder of a tree by id — what
  `sl-viewer-ctl inventory [--library] [--wait-loaded SECS]` prints; the
  viewer's model holds a folder's items once the UI has paged it, so a whole
  tree is read after a search query has swept every folder), `quiescence`
  (outstanding work, bucket by bucket) and `environment`.
- **Streams** — the event log (session events, outbound commands, UI
  actions; sequence-numbered, read by cursor or subscribed to) and the
  warnings and errors logged since a cursor.

## Failures explain themselves

Every error carries the candidates, a tree excerpt around the locator's
scope, the event log's tail and the warnings logged while it ran. The driver
saves them under its artifact directory as `<NNN>-<action>/screenshot.png`
(the locator's matches outlined), `tree.txt` and `events.txt`. Start there:
the screenshot usually says in one glance whether the thing was covered,
scrolled away or never built.

## The two transports

- **In process**: `InProcessHost` builds each viewer App
  (`ViewerAppBuilder`, see the `sl-client-bevy-viewer` assembly) on a thread
  of its own and steps it there continuously, so viewers never take turns;
  `Viewer::over_link` talks to one.
- **Remote**: a viewer started with `--automation-socket [PATH]` serves one
  JSON request per line on a `0600` Unix socket; `Viewer::connect` talks to
  it. Requests that play input need `--headless`.

The driver depends on the protocol and tokio only, so a test chooses the
backend by environment, never by rewriting.

## Headless and watched

`--headless` renders into an off-screen window (a marker the Bevy fork adds,
`OffscreenWindow`): no OS window, no compositor, no desktop input, a private
clipboard, the rendered frame readable as a screenshot. `--watch` adds a second,
view-only window showing that frame to a person. A headless viewer puts no file
chooser on the desktop; the test answers it (`answer_file_dialog`).

## The stage: end-to-end tests

An end-to-end test lives in `sl-client-bevy-viewer/tests/e2e_*.rs` and
builds a `Stage`:

```rust,ignore
StageBuilder::new("about_window")
    .viewer_binary(env!("CARGO_BIN_EXE_sl-client-bevy-viewer"))
    .viewer("Alpha")
    .run(async |stage: &Stage| {
        let alpha = &stage.viewer("Alpha")?;
        let _opened = alpha.menu_path(&["menu-bar-help", "menu-bar-about"]).await?;
        let block = alpha.ui().window("about").test_id("about:info:support-block");
        let _shown = alpha
            .expect(&block)
            .to_contain_text("Simulator version: sl-fake-grid")
            .await?;
        Ok(())
    })
```

`run` starts a fresh fake grid per backend (in process, then process — or
what `SL_E2E_BACKEND` says), an account `Stage <label>` per viewer, waits
until every viewer has logged in and gone quiet, runs the body, and always
tears down: every viewer logs out, and a session left on the grid fails the
test. Artifacts and each viewer's log land under `target/e2e/<test>/`.
`SL_E2E_WATCH=1` opens each viewer process's `--watch` window, so a person
can follow a run. It needs `SL_E2E_BACKEND=process`, since an in-process
viewer is stepped by its host, not by winit. The window shows the frame only,
not the synthetic pointer.

What a test may say about its stage:

- `region(RegionConfig)` — the regions (a fixed `region_id`, a
  `simulator_version`, a `Scenario` seeding the account's inventory through
  `setup_for_agent`); `start_position`; `configure_grid` for the rest;
- `estate_manager(label)`, `skin(id)` (as `--skin`) and `web_media()`;
- `needs(Need::…)` — grid control, content, a dictated grid — so a test
  that cannot run on a grid skips there instead of failing.

Through the stage a test also reaches the grid's side: `stage.agent(label)`
(a session to script an IM or an object update with `with_sim` /
`with_world`, and the events the viewer sent), `stage.grid()` (teleport or
walk an agent over a border), `mark` / `wait_marker`, and `relog(label)`.

`SL_E2E_GRID=opensim|aditi` runs the same tests against a live grid with the
accounts of its credentials file, under nextest's `live` profile (which
never kills a test — a killed test strands its avatars). Tests that need the
fake grid skip there, saying why. Aditi logins share one cooldown with every
other unattended harness.

```sh
cargo nextest run --release -p sl-client-bevy-viewer --test e2e_windows
SL_E2E_GRID=opensim cargo nextest run --release --profile live \
  -p sl-client-bevy-viewer --test e2e_pilot
```

The cheaper tiers speak the same locators: `sl_viewer_automation::in_app`
drives a fixture `App` by hand-stepped frames with the same engine, so a
fixture test's click is strict and actionable too.

## `sl-viewer-ctl`: the driver from a shell

```sh
# A headless viewer on the local OpenSim, held until Ctrl-C.
sl-viewer-ctl launch --credentials credentials.toml --grid localhost --watch

# Or a fresh fake grid and two viewers from a stage file.
sl-viewer-ctl stage two-viewers.toml

# Then, in another shell (the one socket that answers is found by itself):
sl-viewer-ctl tree 'window[test_id=floater:inventory]' --depth 2
sl-viewer-ctl open build-tools
sl-viewer-ctl click 'window[test_id=floater:build] >> radio[name_key=build-tool-move]'
sl-viewer-ctl world touch 'object[name=Door]'
sl-viewer-ctl world ground-double-click 'Next Door' 20,128
sl-viewer-ctl press w --hold 60
sl-viewer-ctl wait 'window[test_id=floater:about]' --for visible
sl-viewer-ctl agent --json
sl-viewer-ctl screenshot frame.png --outline 'button[name_key=build-apply]'
sl-viewer-ctl events --follow --stream command
```

A stage file names a scenario and the viewers:

```toml
scenario = "catalogue"
capture_size = "1280x720"

[[viewer]]
label = "alice"
watch = true

[[viewer]]
label = "bob"
web_media = true
```

Every verb is one driver call and prints text, or JSON with `--json`;
`attach` runs verbs line by line over one connection. `--timeout` sets how
long an action or a wait waits, `--artifacts DIR` keeps a failure's
screenshot, tree and events.

## Gotchas

- **A chord goes to the focus.** After a `fill`, the field has the focus and
  `Ctrl+T` is the field's; open a window with `open_floater` instead.
- **Two windows of one kind open in one place**: the later covers the
  earlier, and an action on a covered node waits for it to be uncovered.
  Drive a window while it is on top, or close the one above.
- **A virtual list's pooled rows count**: an unbound row is a hidden,
  unnamed `listitem`, so name the row you want.
- **The inventory window pages a folder when it is opened**, and its search
  asks for every folder it has not paged; find an item by searching for it.
- **The fake grid runs no physics**: an avatar stands where it is placed, a
  teleport below the ground lands on it, and a walk over a border is
  scripted (`FakeGrid::cross_agent`).
- **A settings file is written in the viewer's own time** — the one thing a
  test polls rather than waits for in the viewer.
