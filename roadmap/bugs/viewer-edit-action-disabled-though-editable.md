---
id: viewer-edit-action-disabled-though-editable
title: The Edit action is greyed out on objects that build mode edits fine
topic: viewer
status: bugs
origin: user report while verifying viewer-underwater-name-tags-not-drawn (2026-08-29)
refs: [viewer-object-pie-enable-fidelity]
---

Context: [context/viewer.md](../context/viewer.md).

The **Edit** affordance reads disabled on objects that can be selected and
edited perfectly well by entering build mode the ordinary way (Ctrl+B / the
toolbar). So the enable predicate disagrees with the thing it is supposed to be
predicting.

Entering build mode is deliberately unconditional — the Build menu's
`Build Tools` command carries no `enabled_when` at all
(`sl-client-bevy-viewer/src/menu_bar.rs:417`) — which is why the tool always
works and only the Edit affordance greys.

## Two concrete candidates

Which one applies depends on **what was right-clicked**; worth pinning that down
first, because the two fixes are unrelated.

### If the object was a worn attachment: a permanently dead menu slice

`ATTACHMENT_SELF_PIE` declares its Edit slice as

```text
label: "Edit", action: "edit", when: Some(UNIMPLEMENTED),
```

(`sl-client-bevy-viewer/src/attachment_menu.rs:358`). `UNIMPLEMENTED`
(`sl-viewer-ui-widgets/src/menu.rs:2866`) is documented as never being pushed
into the live condition set, so the slot can never be enabled, and the pie
renders it `SlotState::Disabled`. The dispatcher has no `"edit"` arm either
(`attachment_menu.rs:847`), so it is inert as well as greyed.

Routing sends any pick whose `summary.attachment` is true to this pie rather
than the object pie (`avatar_menu.rs:1173`), so a worn attachment gets the dead
slice while the in-world object pie's Edit is unconditional
(`object_menu.rs:745`, `when: None`, with a comment noting the reference opens
the tools even on a no-modify object and lets the sim reject what it must).

### If it was an in-world object: a divergent modify predicate

The canonical predicate is `agent_can_modify`
(`sl-viewer-world-api/src/lib.rs:5199`), which ORs the object's flags **up to
the linkset root** and is *optimistic* when the object is untracked. The
gizmos, transform fields, Texture tab, Material tab and prim contents all use
it.

The Object / Features tab controls do not. `edit_params.rs:2338` computes

```text
let has_modify = data.is_some_and(|d| d.update_flags & FLAGS_OBJECT_MODIFY != 0);
```

from the object's **own** `update_flags` only. That differs twice: it never ORs
with the root, so a child prim selected with "Edit linked parts" reads
no-modify even though the modify bit rides the root; and it is pessimistic, so
an untracked object greys everything where `agent_can_modify` would allow it.

Also worth knowing, for a "greyed but the tool works" report: the Texture and
Material tabs additionally require `representative_face(...).is_some()`
(`edit_texture.rs:1194`, `edit_material.rs:1240`), so a tracking gap in
`texture_entry_of` greys those whole tabs even when modify is granted.

## Next step

Establish which affordance and which object kind produced the report, then fix
that one — the attachment slice needs implementing (or removing), whereas the
in-world case is a matter of pointing `edit_params` at `agent_can_modify` like
every other consumer. See also [[viewer-object-pie-enable-fidelity]].
