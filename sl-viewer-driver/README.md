# sl-viewer-driver

The async test API over the viewer automation protocol
(`sl-automation-proto`): what a test, the end-to-end stage or the command line
tool drives a Second Life viewer with — through the same calls whether the
viewer runs in its own process behind an automation socket or in the test's
process.

- `Viewer::connect(socket, options)` reaches a viewer started with
  `--automation-socket`; `Viewer::over_link(requests, messages, options)`
  reaches one an in-process host (`sl_viewer_automation::InProcessHost`)
  runs. A transport is a pair of channels, requests out and messages in; the
  driver numbers the requests and routes each answer to its caller.
- `viewer.ui().window("build").button_key("build-apply").click()` — semantic
  locators with `click`, `double_click`, `right_click`, `hover`, `fill`,
  `press`, `check` / `uncheck`, `select_option`, `drag_to` and the reads
  `text`, `value`, `is_disabled`, `is_checked`, `is_visible`, `count`, `all`.
- `viewer.world().object_named("Door").touch()` — world handles with `touch`,
  `open_pie`, `hover`, `select`, `sit`, `drop_from` and their readouts.
- `viewer.expect(&locator).to_be_disabled()`,
  `viewer.expect_chat().to_contain("hello")` — each a wait evaluated in the
  viewer every frame, with a default timeout overridable per call.
- Probes: the agent, the status bar, the transcripts, the notifications, the
  selection, an inventory folder, quiescence, the event log (by cursor or as a
  stream), the diagnostics, a screenshot.
- A failed action or expectation saves a screenshot with the locator's matches
  outlined, the semantic tree around the scope and the event tail into the
  viewer's artifact directory, and names them in the error.
