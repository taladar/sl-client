---
id: viewer-build-tool-modify-permission-gate
title: Build tools — gate modifications on modify/move permission + grey the
  build window
topic: viewer
status: done
origin: user request (2026-07-26) while reviewing the shift-drag duplicate
refs: [viewer-transform-gizmos, viewer-prim-parameter-editing,
  viewer-prim-texture-editing, viewer-create-shift-drag-duplicate]
---

Context: [context/viewer.md](../context/viewer.md).

Build modifications must fail (with a local-chat error) when the agent lacks
**modify** permission on the object — except **position** and **rotation**,
which stay allowed if the object's "anyone can move" is on (i.e. **move**
permission). And the Build Tools window must grey the controls it cannot use
(still showing the values), the reference viewer's disabled-manipulator /
greyed-panel behaviour (`LLViewerObject::permModify` / `permMove`,
`LLPanelObject`/`LLManip` `canAffectSelection`).

## Done

**Permission source** = the agent-relative `update_flags` the simulator already
computes per agent (OpenSim's `GenerateClientFlags`), carried on every tracked
object. New in `objects.rs`: the `FLAGS_OBJECT_MODIFY` (`1<<2`) /
`FLAGS_OBJECT_MOVE` (`1<<8`) / `FLAGS_OBJECT_COPY` (`1<<3`) constants (now
shared with `edit_params`, which had its own copies), `ObjectState::agent_flags`
(object bits OR-ed with its linkset root's, like `pick_summary`), and
`agent_can_modify` / `agent_can_move` / `agent_can_copy`. Untracked reads
permitted (the simulator arbitrates), so a transient tracking gap never
false-blocks. This is the same signal the reference's `permModify`/`permMove`
read, so "anyone can move" is honoured for free.

**Rule:** position / rotation need **move** (`MODIFY | MOVE`); everything else
needs **modify**.

**Enforcement:**

- **Gizmos** (`gizmos.rs`): a manipulator press is permission-gated before the
  drag starts — a stretch needs modify, a move / rotate needs move. A denied
  press still claims the pointer (so it is not a selection click) but starts no
  drag and posts a `Build Tools: you do not have permission to …` line to the
  local-chat overlay. The shift-drag copy's no-copy check moved onto the same
  `update_flags` signal (unifying with the mask-based `can_copy` shipped in
  [[viewer-create-shift-drag-duplicate]]). One `EditPerm` enum
  (`Modify`/`Move`/`Copy`) drives the check, the notice verb, and
  `selection_lacking`.
- **Numeric transform fields** (`edit_tool.rs`): a commit is gated the same way
  (size → modify, position / rotation → move); denied posts the notice and the
  field re-syncs to the unchanged value. The nine fields (and their row labels)
  grey **per row**: position / rotation grey on no-move, size on no-modify.
- **Object / Features tab** (`edit_params.rs`): every parameter edit is a
  modify, so `has_modify` folds into the widget `enabled_for` gate — the whole
  tab greys on no-modify.
- **Texture tab** (`edit_texture.rs`): the panel gate now ANDs modify
  permission, greying every control on no-modify.

Notices reuse the client-side `chat::LocalChatNotice` overlay message added in
[[viewer-create-shift-drag-duplicate]]. Live-verified on OpenSim against a
non-owned prim (greyed fields + the no-permission notice) and an owned prim
(fully editable).

**Deliberately out of scope (follow-up
[[viewer-build-material-tab-permission-gate]]):** the **Material** tab
(`edit_material.rs`) — it has no control-disable / greying infrastructure yet
(it does not grey even on an empty selection) and several separate commit
systems, so its gate is its own task. A no-modify material edit is still
rejected by the simulator; only the client-side grey / notice is pending.
