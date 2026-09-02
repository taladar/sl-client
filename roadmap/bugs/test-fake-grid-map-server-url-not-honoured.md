---
id: test-fake-grid-map-server-url-not-honoured
title: The viewer falls back to map.secondlife.com instead of the grid's map-server-url
topic: test
status: bugs
origin: Firestorm cross-check harness run (2026-09-02)
points: 2
refs: [test-firestorm-crosscheck-runner]
---

Context: [context/testing.md](../context/testing.md).

Logged into the fake grid, Firestorm fetches its world-map tile from
**`https://map.secondlife.com/`** rather than from the fake grid, and
fails:

```text
doWork : HTTP GET failed for: map-1-1000-1000-objects.jpg
         Status: Easy_6 Reason: 'Couldn't resolve host name'
stageAfterCompletion : HTTP request failed after 5 retries. (Easy_6)
```

`https://map.secondlife.com/` is the value of Firestorm's
`CurrentMapServerURL` setting — its **fallback**, used when the region's
`SimulatorFeatures` carries no `map-server-url` in `OpenSimExtras`
(`lfsimfeaturehandler.cpp:103`). So the viewer never saw the grid's own.

The fake grid does set it, in both places it should
(`runtime.rs:404` for the login response's `map-server-url`, `:595` for
the `SimulatorFeatures` `OpenSimExtras`), and both are the login URI —
which is a loopback address needing no DNS at all. So the value is being
produced but is not reaching the viewer: either the `SimulatorFeatures`
cap is not being fetched, or its `OpenSimExtras` block is not in the
shape `lfsimfeaturehandler` reads. Worth confirming which by fetching
the cap directly and comparing against what OpenSim sends.

This is the last thing keeping a fake-grid session from reaching
**quiescence**. Five retries against an unresolvable host keep the
texture-fetch queue non-empty for the whole session, so
[[test-firestorm-crosscheck-runner]]'s captures fire on the settle
timeout rather than at a settled scene — which is exactly the
uncontrolled variable a frame-to-frame comparison must not have. The
other twenty missing textures were fixed by vendoring OpenSimulator's
real ones; this one is not a missing asset but a misrouted request.

An offline machine makes it worse but is not the cause: pointing at a
host the grid never nominated is wrong even when that host resolves,
because the tile then comes from Second Life's map rather than from the
region under test.
