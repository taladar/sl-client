# Scripts & Permissions

LSL scripts live inside the inventory of in-world objects ("tasks"). A viewer
does not run the scripts itself, but it can drive their lifecycle — query
whether a script is running, start or stop it, reset it — and it must answer the
dialogs and permission requests scripts raise. This chapter covers the
task-script control surface and the existing dialog/permission events.

## Task inventory

An in-world object ("task") has its own inventory — the scripts, notecards,
animations, and other items dropped inside it. Four UDP commands read and mutate
it, each targeting the object by [`ScopedObjectId`](world.md#objects):

- `Command::RequestTaskInventory { target }` asks for the contents listing; the
  reply is `Event::TaskInventoryReply(TaskInventoryReply)`, giving the contents
  `serial` (it increments on every change, so a client can tell whether a cached
  listing is stale) and the temporary Xfer `filename` to download the full
  listing from (empty when the task inventory is empty).
- `Command::FetchTaskInventory { target }` goes one step further: it sends the
  same request, then downloads and parses the Xfer listing the reply names,
  surfacing the parsed items as `Event::TaskInventoryContents` (the
  `TaskInventoryReply` is still emitted first). See [Xfer File
  Transfer](../comms/xfer.md) for the download mechanism and the listing format.
- `Command::UpdateTaskInventory { target, key, item }` writes (adds or replaces)
  an item. `key` is a `TaskInventoryKey` choosing whether the simulator matches
  the existing entry by item id or by asset id; `item` is the full inventory
  item (a `RestoreItem`, shared with the rez paths).
- `Command::MoveTaskInventory { target, folder_id, item_id }` moves an item out
  of the object's inventory into an agent inventory folder.
- `Command::RemoveTaskInventory { target, item_id }` deletes an item from it.

## Rezzing & revoking

- **Drop a script into an object.** `Command::RezScript { target, params }`
  drops a script inventory item into an in-world object's task inventory
  (`RezScript`). The `RezScriptParams` carries the running flag, the active
  group, and the source item.
- **Revoke granted permissions.** `Command::RevokeScriptPermissions { object_id,
  permissions }` revokes LSL permissions previously granted to an object
  (`RevokePermissions`) — the inverse of the permission grant below. It is
  object-scoped (by `ObjectKey`, not the per-circuit local id), so it applies
  wherever that object is.

## Task-script control

The full list of a task's scripts (and its other inventory) also comes from the
`RequestTaskInventory` CAPS path covered in [Inventory](inventory.md). On top of
that listing, three messages drive an individual script, each identifying the
script by the task's id (`object_id`) and the script inventory item inside it
(`item_id`):

- **Is it running?** `GetScriptRunning` (`Command::RequestScriptRunning`) asks
  the simulator whether a script is active. Unlike most viewer→sim messages it
  carries no `AgentData` block — just the object and item ids. The simulator
  answers with `ScriptRunningReply` (`Event::ScriptRunning`), reporting the
  `running` flag.
- **Start / stop.** `SetScriptRunning` (`Command::SetScriptRunning`) sets the
  run state: `running = true` starts the script, `false` stops it.
- **Reset.** `ScriptReset` (`Command::ResetScript`) resets a script to its
  initial state, as if it had just been (re)compiled — globals back to their
  defaults, `state_entry` re-run.

> **Scope note — script sensors.** `ScriptSensorRequest`/`ScriptSensorReply`
> (the `llSensor` family) are `Trusted` simulator↔dataserver messages: the
> viewer never sends or receives them, and neither the Firestorm viewer nor
> OpenSim's `LLClientView` handles them. Following the same trusted-backend
> precedent as earlier tiers, they are **not** wrapped; they remain reachable as
> a raw `AnyMessage` if ever needed.

## Dialogs & permission requests

Scripts interact with the agent through two inbound events the viewer must be
ready to answer:

- **Dialogs** (`llDialog`/`llTextBox`): the simulator sends `ScriptDialog`
  (`Event::ScriptDialog`) with the message text and the button labels. The
  viewer replies with the chosen button via `ScriptDialogReply`
  (`Command::ReplyScriptDialog`) on the dialog's hidden chat channel.
- **Permission requests** (`llRequestPermissions`): the simulator sends
  `ScriptQuestion` (`Event::ScriptPermissionRequest`) listing the permissions a
  script wants (take controls, animate the agent, attach, debit, …). The viewer
  grants a subset with `ScriptAnswer` (`Command::AnswerScriptPermissions`).

## Uploading & compiling a script

Editing a script's source is **not** a plain asset upload: the viewer never
compiles LSL (or Lua) locally — it POSTs the raw source to a capability and the
**simulator compiles it synchronously** and returns the result. Two capabilities
carry the source, chosen by where the script lives:

- **Agent inventory** — `UpdateScriptAgent`, carrying the `item_id`.
- **Object task inventory** — `UpdateScriptTask`, carrying `task_id`, `item_id`,
  `is_script_running`, and an optional `experience`.

Both also carry a **compile target** — the `target` field, one of `mono`,
`lsl2`, or `luau`. This is a *request* for a backend, not a local action: Second
Life honours it (LSL Mono, legacy LSL, or Lua/SLua via Luau), while OpenSim
ignores it and picks the language from a source-header comment. Lua and LSL
share the same script asset type; the language is also recorded as an inventory
*subtype* in the low byte of the item flags (`Lsl` = 0, `Luau` = 1).

The completion reply carries the simulator's compile outcome: a `compiled`
boolean and an `errors` array of diagnostic strings. A script can upload
successfully as an asset yet fail to compile — that is not a transport
failure.

Creating a brand-new script goes through `CreateInventoryItem` (carrying the
language subtype): the **simulator fills a compilable default body** (the LSL
default, or the Lua default on a SLua-capable grid) — never an empty,
non-compiling script. Replace that body with custom source (and compile it) with
a follow-up upload.

> **In this codebase**
>
> - Upload: `Command::UploadScript { location, target, source }`, where
>   `location` is `ScriptUploadLocation::AgentInventory { item_id }` or
>   `::TaskInventory { task_id, item_id, running, experience }`, and `target`
>   is the `#[non_exhaustive]` `ScriptTarget` (`Lsl2` / `Mono` / `Luau`). The
>   result is `Event::ScriptUploaded { new_asset, new_inventory_item, compiled,
>   errors, running }`; a transport failure is `Event::AssetUploadFailed`
>   instead.
> - Compile diagnostics are parsed into `ScriptCompileError { raw, line, column,
>   message }` (the `raw` string is always preserved; the position is a
>   best-effort parse of the grid's format).
> - Create: `Command::CreateScript { folder_id, name, description,
>   next_owner_mask, language }` (`ScriptLanguage::Lsl` / `Luau`), backed by
>   `Session::create_script`; the reply is `Event::InventoryItemCreated`.
> - The generic asset-upload commands (`UploadAsset` / `UpdateInventoryAsset`)
>   **cannot** carry a script — `UpdateInventoryAsset` takes an
>   `UpdatableAssetType` with no script variant, so a script update through the
>   compile-blind path is a compile error, not a silent discard.
> - The two capabilities are `CAP_UPDATE_SCRIPT_AGENT` /
>   `CAP_UPDATE_SCRIPT_TASK`; the request builders are
>   `build_update_script_agent_request` /
>   `build_update_script_task_request` in `sl-wire/src/llsd.rs`; the two-step
>   upload runner is `run_script_upload` in each runtime crate's `upload.rs`.
> - REPL commands `create_script`, `upload_script_agent`, `upload_script_task`.

## Server side

A simulator built on `SimSession` decodes each inbound task-script message into
a matching `ServerEvent` (`RequestScriptRunning`, `SetScriptRunning`,
`ResetScript`) and answers a run-state query with
`SimSession::send_script_running_reply`. The task-inventory, rez, and revoke
messages decode the same way — `ServerEvent::RequestTaskInventory` /
`UpdateTaskInventory` / `MoveTaskInventory` / `RemoveTaskInventory`,
`ServerEvent::RezScript`, and `ServerEvent::RevokeScriptPermissions` — and a
`RequestTaskInventory` is answered with `SimSession::send_reply_task_inventory`.

---

> **In this codebase**
>
> - Commands `RequestScriptRunning`, `SetScriptRunning`, `ResetScript`; the
>   `Session` methods are `request_script_running`, `set_script_running`,
>   `reset_script`; the wire encoders are `send_get_script_running`,
>   `send_set_script_running`, `send_script_reset` in
>   `sl-proto/src/session/circuit.rs`.
> - Event `ScriptRunning` is decoded in `sl-proto/src/session/methods.rs`.
> - Server events `RequestScriptRunning`, `SetScriptRunning`, `ResetScript`,
>   plus `send_script_running_reply`, are in `sl-proto/src/sim_session.rs`.
> - REPL commands `request_script_running`, `set_script_running` (the `running`
>   flag accepts `true`/`false`), and `reset_script`.
> - The existing dialog/permission surface is the commands `reply_script_dialog`
>   and `answer_script_permissions` and the events `ScriptDialog` /
>   `ScriptPermissionRequest`; `RevokeScriptPermissions` (helper
>   `revoke_script_permissions`) is the inverse of `answer_script_permissions`.
> - Task inventory: commands `RequestTaskInventory`, `UpdateTaskInventory`,
>   `MoveTaskInventory`, `RemoveTaskInventory` (helpers of the same snake-case
>   names) in `sl-proto/src/command.rs`; the reply is
>   `Event::TaskInventoryReply(TaskInventoryReply)`
>   (`sl-proto/src/types/object.rs`). `TaskInventoryKey` and `RezScriptParams`
>   are in `sl-proto/src/types/editing.rs`; the rez/update payload reuses
>   `RestoreItem`. REPL tokens `request_task_inventory`,
>   `update_task_inventory`, `move_task_inventory`, `remove_task_inventory`,
>   `rez_script`,
>   `revoke_script_permissions`.
