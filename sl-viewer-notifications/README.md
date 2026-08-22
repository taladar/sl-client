# sl-viewer-notifications

The viewer's notification catalogue: every notification the client can raise,
declared once as data, plus the queue a host UI drains.

A notification here is a `NotificationTemplate` — a stable identifier, a
severity, a Fluent message key, and the buttons it offers. Nothing in this
crate draws anything. Code that wants to tell the user something writes a
`ShowNotification` message naming a template and its arguments; the viewer's
notification host resolves the key through Fluent, lays the toast or dialog
out, and reports the response back.

Keeping the catalogue separate from the host is what makes it reviewable: the
question "what can this viewer say to me, and what will it ask?" is answered by
reading one data table rather than by chasing calls through the UI.

## Why its own crate

The catalogue is ~22k lines of declarative data with no dependency on anything
else in the viewer — it names no module, and only the `Message` and `Resource`
derives connect it to Bevy at all. That made it the cheapest piece of the
viewer to compile separately, and it rarely changes, so the crates that depend
on it almost never have to rebuild because of it.

## Layout

- `NOTIFICATIONS` — the catalogue, one `NotificationTemplate` per entry.
- `NotificationManager` — the queue: what has been raised, what is showing, what
  the user answered.
- `substitute` / `template` — argument substitution into a resolved message.

The Fluent bundles the message keys resolve against live with the viewer, which
owns the shipped locale assets; a test there checks every key in this catalogue
exists in the English bundle.
