# sl-viewer-automation

The viewer side of automation: a **semantic model** of the UI, read from the
ECS when asked and never per frame, in the vocabulary of
[`sl-automation-proto`](../sl-automation-proto).

Each node the model reports has a role inferred from its widget components
(button, checkbox, radio, slider, text field, text, image, and named groups),
an accessible name (an `AccessibleLabel` override, else the Fluent key and the
text resolved from it, else the descendant text), its states (disabled —
inherited from any ancestor —, read-only, checked, selected, focused,
hovered), its value, its bounds in logical pixels, and whether it can be seen
(hidden, clipped out of a scroll area, off screen, or covered by another node).

The same model is what a screen reader needs, so it is built once and serves
both — but it depends on no AccessKit, which exists only under a window.

Beside it sits a **world model**: the objects, avatars and attachments the
viewer's world layers already track, read as world nodes (ids, name, owner,
region-local placement, link set, attachment point, sit state, selection,
floating text and name tag). World locators resolve against it — by kind, the
own avatar, name, id, owner, object class, floating text and proximity — and a
world query waits for the object names and owners the simulator sends only
when asked, asking for them itself.
