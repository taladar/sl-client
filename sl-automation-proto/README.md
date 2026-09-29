# sl-automation-proto

The vocabulary every driver of a Second Life viewer agrees on: the in-process
and remote automation transports, the driver library, the command line tool
and — one day — the patched Firestorm. Pure data with serde, no runtime and no
Bevy.

- `Locator` names a UI node **semantically** — a role plus an accessible name
  (preferably the locale-independent Fluent key), or a test id, scoped with
  `within` and narrowed by state filters. Coordinates never appear.
- `UiNode` is one node of a semantic snapshot: role, name, name key, test id,
  states, value, bounds in logical pixels and visibility.
- `WorldLocator` names an in-world thing the same way — an object, an avatar,
  the own avatar or a worn attachment, by name, id, owner, object class,
  floating text and proximity — and `WorldNode` is what the viewer reports for
  one.
- State probes read what a test asserts on that is not one widget: chat and
  instant-message transcripts, notifications and the buttons they offer, the
  status bar, the own agent (region, position, seat, teleport, camera), the
  selection, an inventory folder by path, sequence-numbered logs of events,
  commands and UI actions read by cursor, the warnings and errors logged, and
  whether the scene has settled.
- `Request` / `Response` carry an id, so several may be in flight.
- `AutomationError` is what a test matches on: not found, ambiguous (with every
  candidate), not actionable (with the failing check) and timed out (with the
  last observed state), for UI and world locators alike.

The *types* are shared, but a Fluent key or a test id means something only to
the viewer that owns it: selectors are a per-viewer namespace.
