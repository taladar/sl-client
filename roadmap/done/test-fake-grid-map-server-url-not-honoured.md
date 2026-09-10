---
id: test-fake-grid-map-server-url-not-honoured
title: The reference viewer builds its first map-tile URL before it knows the map server
topic: test
status: done
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

Done (2026-09-09) — **and the grid was innocent.** The report above names
the wrong host and therefore the wrong side of the wire; both are worth
keeping, because the correction is the whole lesson.

**What the fake grid actually does.** Probed directly against a running
grid, with the reference viewer's own `options` list:

```text
login-response  map-server-url = http://127.0.0.1:9137/
GET /map-1-1000-1000-objects.jpg -> 200 image/jpeg, 1553 bytes
GET /map-1-1001-1000-objects.jpg -> 404          (no such region)
```

and Firestorm's own log agrees, at the top of every run since:

```text
process_login_success_response : map-server-url : we got an answer from
the grid : http://127.0.0.1:9143/
```

So the value is produced, sent, requested, received *and* stored. The
`SimulatorFeatures` half is not the missing link either: the stock grid
imitates Second Life, which sends no `OpenSimExtras` block at all, and
`lfsimfeaturehandler.cpp` falls back to `CurrentMapServerURL` — the field
just processed — in exactly that case.

**The evidence that redirects the diagnosis** is one word in the failing
line. `lltexturefetch.cpp:1782` logs `mUrl`, so what it printed was the
*whole* URL:

```text
doWork : HTTP GET failed for: map-1-1000-1000-objects.jpg
```

No scheme, no host. `map.secondlife.com` was never involved; the report
inferred it from `CurrentMapServerURL`'s default without reading the line
as a URL. `LFSimFeatureHandler::mapServerURL()` returned the **empty
string**, and a hostless URL is a host name to curl, which is why an
offline machine reported it as DNS.

**Why it was empty.** `mMapServerURL` is a plain `std::string` the
constructor never seeds; only `setSupportedFeatures()` ever writes it. The
constructor calls that itself, but at that moment `gAgent.getRegion()` is
still null, so it returns having written nothing — and the region's own
surface asks for its world-map tile *while the region is being created*
(`LLSurface::createSTexture` → `LLWorldMipmap::loadObjectsTile`), which is
before the region exists to be found. The log orders it plainly:

```text
1217  process_login_success_response : map-server-url : ...9145/
1262  LFSimFeatureHandler : Initializing Sim Feature Handler
1283  addRegion : Add region with handle: ...     <- tile URL built here
1328  setSupportedFeatures : Setting defaults...  <- mMapServerURL set here
```

Firestorm's minimap overlay (`LLViewerRegion::getWorldMapTiles`) builds
its URL the same way and caches the result per region, so it has the same
bug; the surface is simply the one a capture run with no UI reaches.

**Fixed in the fork** (`phoenix-firestorm`, branch `test-harness`): the
constructor seeds `mMapServerURL` from `CurrentMapServerURL`, which the
ordering above shows is already the grid's own answer by then. One line
plus its reason, in the `.cpp` alone — the header has 33 includers and
this does not need to touch them.

Verified by a `--scenario catalogue` cross-check run, against the run that
filed this. The five-retry failure and the ~32 s it held the fetch queue
open are gone, and no tile fetch fails at all:

```text
before   HTTP GET failed for: 1     retry-exhausted requests: 6
after    HTTP GET failed for: 0     retry-exhausted requests: 5
```

The five that remain are immediate 403/404s that retry zero times, so
nothing keeps the queue non-empty any more — which was what
[[test-firestorm-crosscheck-runner]] needed from this.

**Kept from the original report**, because it was right and was never
done: a grid that answers `login: true` while omitting a field the
reference viewer treats as mandatory is worse than one that refuses.
`LoginSuccess::missing_required_fields` is that check — Firestorm's own
five-way test at the end of `process_login_success_response`, written
where a grid can run it — and `sl-fake-grid` now refuses such a login and
names the field instead of building the response. It is checked on the
response the grid *built*, before `filter_options` trims it, because
`inventory-root` is both mandatory to the viewer and legitimately absent
when the client never asked for it.

The lesson worth carrying: read the URL a log prints *as a URL*. A missing
host and a wrong host summarise identically and are not the same bug —
one is the grid's, one is the viewer's, and this cost a day of looking at
the grid.
