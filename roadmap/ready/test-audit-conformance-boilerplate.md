---
id: test-audit-conformance-boilerplate
title: Factor the repeated session/id acquisition out of the conformance cases
topic: test
status: ready
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/test.md](../context/test.md).

Per-case ceremony is tolerable (~20 lines of `impl GridTest` header plus 127
`wait_for_region` call sites, about 8% of a 250-line file). The one large
duplication is session and id acquisition:

- **71 identical occurrences** across 20 files of
  `ctx.secondary().ok_or_else(|| TestFailure::Assertion("two-account test ran
  without a secondary".to_owned()))?` (`chat_hear_other.rs:70`,
  `im_1to1.rs:74`, `:97`, ...);
- **30 copies** of
  the agent-id form:

  ```text
  agent_id().ok_or_else(|| TestFailure::Assertion(
      "... did not report an agent id"))
  ```

`support.rs` already factors out timeouts, `check` / `check_eq`, metric names,
`send_then_wait`, `membership_group` and `confirm_group_departure` — but nothing
for this. The missing helpers:

```text
impl TestContext {
    pub fn secondary_or_fail(&mut self) -> Result<&mut Session, TestFailure>;
    pub fn tertiary_or_fail(&mut self) -> Result<&mut Session, TestFailure>;
    /// wait_for_region on primary + every configured peer, concurrently.
    pub async fn all_in_region(&mut self, timeout: Duration) -> Result<(), TestFailure>;
}
impl Session {
    pub fn require_agent_id(&self) -> Result<AgentKey, TestFailure>;
    pub fn require_session_id(&self) -> Result<Uuid, TestFailure>;
    pub fn require_region_handle(&self) -> Result<RegionHandle, TestFailure>;
}
```

That deletes roughly 200 lines across 20 files and removes the strongest reason
a case runs to 450 lines.

Second, unrelated to boilerplate but the same file set: **33 fixed `sleep`s
across 26 case files** instead of waiting on a condition. The worst are the
unconditional settles — `parcel_divide_join.rs:50` (`EDIT_SETTLE = 2s`, slept
three times), `group_accounting.rs:78` / `group_roster.rs:55` /
`group_admin.rs:64` (`CREATE_SETTLE = 2s`), `group_session_message.rs:84`
(`SESSION_SETTLE = 3s`), `grant_user_rights.rs:71`, `:80`, and three bare
`sleep(500ms)` in `script_running.rs` / `script_upload.rs`. Each is a coin flip
on a loaded aditi region. (The `VERIFY_POLL_INTERVAL` sleeps inside bounded poll
loops are fine.)

Third: **17 cases depend on grid state the case does not provision** — every
file with a `start_location` override needs the `SLClientScriptTester` prim from
a hand-loaded OAR, which lives nowhere in git. A fresh checkout cannot run them
and there is no precondition check with a useful message.
