---
id: viewer-automation-mcp-server
title: The automation verbs as an MCP server for agent sessions
topic: viewer
status: ideas
origin: viewer automation design (2026-09-28)
points: 3
blocked_by: [viewer-automation-ctl-cli]
---

Context: [context/automation.md](../context/automation.md).

`sl-viewer-ctl` already lets an agent drive a viewer through the shell. An
MCP server exposing the same verbs as typed tools (with screenshots
returned as images) would save the agent from parsing CLI output and keep
one long-lived connection per viewer.

Open question before promoting: whether the CLI's `--json` output is
already enough in practice. Build this only if agent sessions show the CLI
to be the bottleneck.
