---
id: viewer-automation-ctl-drag-verbs
title: sl-viewer-ctl — drag verbs (onto a node, by an offset)
topic: viewer
status: done
origin: test-e2e-sweep-relog (2026-10-01)
points: 2
refs: [viewer-automation-ctl-cli, test-e2e-sweep-relog]
---

Context: [context/automation.md](../context/automation.md).

The driver has two drags that `sl-viewer-ctl` cannot play:
`UiLocator::drag_to` (`RequestBody::DragTo`, a node dropped onto another —
an inventory row onto a folder) and `UiLocator::drag_by`
(`RequestBody::DragBy`, a node dragged by a relative offset — a window by
its `floater-title-bar`, or resized by its `floater-resize`). Both are what
a person or an agent driving a viewer from the shell reaches for: arranging
windows before a screenshot, filing an item, reproducing a drag bug.

- A `drag` verb in `sl-viewer-ctl/src/cli.rs` taking the source selector
  and exactly one of `--onto <selector>` or `--by <x>,<y>` (logical pixels,
  `x` rightwards and `y` downwards; a negative offset must parse, so not as a
  bare positional), and its arm in `verbs.rs` calling the matching driver
  method.
- An `Outcome` printing the node answered — the target for `--onto`, the
  dragged node as pressed for `--by` — as text and JSON.
- Usable from `attach` like every other verb.
- Scripted-viewer tests in `sl-viewer-ctl/src/tests.rs`: each form sends the
  request it should, the two flags are refused together and when both are
  missing, and a malformed offset is refused before anything is sent.
- The verb list in the context file's `sl-viewer-ctl` entry and the
  crate's docs gain `drag`.

## Done (2026-10-01)

- `drag <selector> --onto <selector> | --by X,Y` (`cli.rs`, a required
  one-of group; `--by` takes hyphen values, so `--by -40,0` parses, and
  `drag_offset` refuses anything but two finite numbers). It prints
  `dropped onto <target>` or `dragged <node as pressed>`, `"done"` in JSON.
- Scripted-viewer tests: each form's request, a negative offset in both
  spellings, JSON output, and the refusals (both flags, neither, four
  malformed offsets) at parse time, before anything is sent.
- **Acceptance, from a shell** (release build, `stage` on the fake grid):
  the Inventory window moved by its `floater-title-bar` (`--by -60,40`),
  resized by its `floater-resize` (`--by=30,-20`), and its title bar dropped
  `--onto` the toolbar's Build button; each read back from the window's
  bounds.
