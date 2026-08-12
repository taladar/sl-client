---
id: test-recommit-conformance-records
title: Revisit committing sl-conformance records once implementation churn settles
topic: test
status: deferred
origin: user decision (2026-08-12) — records/ is deliberately not committed
  during the implementation phase
---

Context: [context/test.md](../context/test.md).

The `records/<grid>/<test>.toml` files are no longer committed, and
`records/` is gitignored: during the current high-churn implementation phase
— where the project owner is the only user — nearly every commit is
behaviour-relevant and stales all records, so committing them added churn
without signal. Runs still write records locally, and
`sl-conformance-report` still reads them; they are just not versioned.

Once the implementation phase settles (release users exist,
behaviour-relevant commits become rarer), revisit this decision: remove
`/records/` from `.gitignore` and resume the one-commit-per-case convention
(case + record + roadmap together), starting the committed record history
from that point.
