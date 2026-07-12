---
id: missing-eq-batch-2
title: group & display-name pushes
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**EQ batch 2 — group & display-name pushes.** `AgentDropGroup` (the sim
dropped the agent from a group), `DisplayNameUpdate` (a cached display name
changed), `SetDisplayNameReply` (result of the agent's own set-display-name).

Implemented as `Event::AgentDroppedFromGroup { group: GroupKey }` (an inline
variant — the echoed `AgentID` is this agent itself and is dropped, leaving
only the `GroupID` the sim removed the agent from), and two boxed variants
carrying domain structs in the new `types/display_name.rs`:
`Event::DisplayNameUpdate(Box<DisplayNameUpdate>)` where `DisplayNameUpdate {
old_display_name: String, name: DisplayName }` reuses the existing
`sl_wire::DisplayName` record (the push's `agent` block is
`LLAvatarName::asLLSD`, the same People API fields as a `GetDisplayNames`
entry but with no embedded `id` — so the id is taken from the body's top-level
`agent_id`); and `Event::SetDisplayNameReply(Box<SetDisplayNameReply>)` where
`SetDisplayNameReply { status: i32, reason: String, new_display_name:
Option<String>, error_tag: Option<String> }` extracts the meaningful fields of
the polymorphic `content` blob (the new name on success, the error tag on
failure) and exposes a `succeeded()` helper (`status == 200`). All three are
SL-only — OpenSim never pushes them. Decoded by `agent_drop_group_from_llsd` /
`display_name_update_from_llsd` / `set_display_name_reply_from_llsd` in
`session/conversions.rs` and dispatched by name in
`Session::handle_caps_event`.
