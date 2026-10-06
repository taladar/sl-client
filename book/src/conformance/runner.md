# The Runner

`sl-conformance` runs exactly one test against one grid per invocation.

## Commands

```text
sl-conformance run    --grid <opensim|aditi|fake-sl|fake-opensim>
                      [--avatar <name>] [--secondary <name>]
                      [--credentials <path>]
                      [--fixtures <path>] [--force] [--timeout <secs>]
                      <TEST>
sl-conformance run-offline [--grid <fake-sl|fake-opensim>]
sl-conformance list   [--grid <opensim|aditi|fake-sl|fake-opensim>]
sl-conformance generate-manpage --output-dir <dir>
sl-conformance generate-shell-completion --output-file <f> --shell <shell>
```

- `run` takes a single positional `TEST`. There is no batch form for a live
  grid: running tests one at a time is the primary safeguard against aditi
  rate-limiting.
- `run-offline` is the batch form for the fake grid, which has no such limit:
  it runs every offline case on both flavours (or the one `--grid` names) and
  records each run, which is what fills the fake columns of the report (see
  [Live and fake side by side](#live-and-fake-side-by-side)).
- `list` shows the registered tests, the grids each applies to, and how many
  avatars each needs.

## Grid and avatar selection

`--grid` chooses the target. The credentials file defaults to `credentials.toml`
for OpenSim and `credentials.aditi.toml` for aditi; override with
`--credentials`. The primary avatar comes from `--avatar` (or the file's default
avatar).

`--grid fake-sl` (or `fake-opensim`) needs none of that. It stands an
`sl-fake-grid` up inside the runner process on ephemeral ports, imitating the
grid the flag names, registers three accounts and synthesises the
credentials that reach them, so there is no file to write, no cooldown to
respect and no network to be on. It exists for the same reason the runner has a
`--timeout`: to run *one* offline case by hand with the full trace log, when
the `cargo test` suite says it fails and you want to watch it. See
[The offline grid](#the-offline-grid) below.

### The avatar-availability precondition

Before any login, the runner checks that the credentials provide enough distinct
avatars for the test, and refuses *only* when they do not — so a single
configured avatar still runs every one-account test. A two-account test needs a
distinct secondary, resolved as:

1. `--secondary <name>`, else
2. the conventional `[avatars.secondary]` entry, else
3. the first other avatar in the file with a different `First Last` identity.

If none can be resolved, the run is refused before any network activity, naming
the required versus found count.

## Fixtures (pre-made grid resources)

Some cases need a *stable, pre-existing* grid resource rather than one created
fresh each run. The membership/messaging group cases are the motivating example:
on the throwaway OpenSim grid, creating a group per run is free and disposable,
but on Second Life creating a group costs **L$100**, an emptied group purges
only after ~48&nbsp;h, and the founder holds a group slot for every group they
create — so a case that creates per run both spends L$ and marches the founder
toward Second Life's ~42-group cap.

To avoid that, such a case reads an optional, gitignored fixtures file
(`fixtures.toml` for OpenSim, `fixtures.aditi.toml` for aditi; override with
`--fixtures`). It lists pre-made groups the primary owns; a case takes the
group(s) it needs **by position** — the membership/messaging cases use the
first, while `chat-invite-accept-decline` uses the first two (it needs two
distinct pending sessions). When a group is configured at the position a case
asks for, the case reuses it; otherwise it creates a throwaway. A case that
joins a reused group also leaves it again, so the fixture is left as it was
found (a fresh join is also what makes the invitation case fire).

```toml
# fixtures.aditi.toml — pre-made open-enrollment groups the primary owns.
premade_groups = [
  "00000000-0000-0000-0000-000000000000",
  "11111111-1111-1111-1111-111111111111",
]
```

The cases that rez objects (they say so with `GridTest::rezzes_objects`)
read one more fixture, `build_location`, a login `start` string such as
`"uri:Mauve&52&62&33"` naming a spot where everyone may build. On Second
Life the last location is often land that refuses a rez, a region's telehub
redirects logins and teleports alike, and a rez far from the avatar is
dropped without a word, so such a case logs in there, **flies** the avatar to
the spot when the login landed elsewhere, points the camera at it (objects
are streamed by where the camera is), and rezzes there. Absent, the case
keeps its own start location and records `partial` with the grid's refusal
when the rez is refused.

`parcel-ban-line` reads `ban_line_regions`, a list of region names. The case
needs a parcel that is closed to strangers and looks for one where the login
lands; a region without one is left for the next name on the list, by a map
lookup and a teleport to its middle. Absent, only the login region is looked
at, and a run that finds no such parcel is recorded `partial`.

A case recognises the object it rezzed by its ownership — the per-viewer
`OBJECT_YOU_OWNER` flag, since Second Life sends a plain prim's owner id as
nil — never as "the next object that appeared": a sandbox streams other
residents' rezzes all the time.

Every field is optional and an absent file is equivalent to an empty one, so no
fixtures file is needed to run on OpenSim.

## The offline grid

The fake grid is a third and fourth target, and the only ones that run without
anyone standing a grid up. `sl-conformance::fake` starts an `sl-fake-grid`
serving two regions — the shared fixture catalogue (`Fake Region`) and the
border scene east of it (`Fake Region East`), announced as its neighbour —
registers the
`primary` / `secondary` / `tertiary` accounts, and hands out the login URI it
bound as synthesised credentials. Everything below that is the ordinary login
path, XML-RPC round trip included.

The point is `sl-conformance/tests/offline.rs`: one `#[tokio::test]` per name in
`fake::OFFLINE_CASES` **and per flavour** (`object_edit::fake_sl`,
`object_edit::fake_opensim`), each on its own fresh grid. Those cases are
therefore exercised on **every** `cargo test` — and so on every commit — instead
of the next time somebody remembers to log a live grid in. Unit tests pin the
list against the registry in both directions, so a case cannot declare *a* fake
grid without being run, nor be listed without declaring one, and pin the tests
in that file to exactly the flavours each case declares.

Two rules decide whether a case belongs there:

- **Every fixture it needs is offline.** A case asserting protocol *shape* — a
  handshake, a ping, a throttle, a parcel record, the world map — qualifies. One
  asserting *grid semantics* (groups, money, experiences, the marketplace) does
  not, and neither does one whose provocation the fake grid has no answer for:
  it would pass by recording `partial`, which costs suite time to assert
  nothing.
- **It bites offline.** Where a case branches on `is_opensim` to *require* what
  a region it controls must contain, use `support::content_is_ours`, which is
  true of OpenSim and the fake grid and false of Second Life.

The second rule is about the grid, not the case, so a case joins the list when
the grid learns to answer it. `agent-alert` and `server-error` were both listed
here as examples of cases that *pass* offline while asserting nothing; each now
asserts, because the fake grid grew the policy behind its provocation — an
estate-rights check and a `FeatureDisabled` refusal of the deprecated UDP
inventory fetch. `task-inventory` was the last of them, and needed more than a
policy: it is the one case that asks the grid to *write*, and it joined the list
once the fake grid grew a
[region-scoped world and a write path](../tools/fake-grid.md#the-write-path).
If a case belongs offline and does not bite, the thing to fix is usually the
grid.

Five cases live on the fake grid **only**, because nothing else can host them:
`region-crossing` and `neighbour-child-circuits` need two adjacent regions an
avatar may walk between; `terrain-layerdata` and `avatar-appearance-npc` assert
against ground and bakes this workspace declares; and `asset-round-trip` walks
the fake grid's own seeded inventory, one item per writable asset class, which
no live account has. That last one is fake-only for a second reason worth
keeping straight: it asserts a save comes back **byte for byte**, which is what
the fake grid implements and very probably *not* what a real grid returns for
several classes — measuring that is `test-asset-save-mutation-survey`'s job,
and the assertion tightens when it has. It is also the one case that runs on
`Grid::FakeOpensim` rather than `Grid::FakeSl`, because its fourth leg reads a
taken object's asset back and Second Life never lets a viewer do that — see
[which grid the fake one is](#which-grid-the-fake-one-is) below. The first of
those also needs the harness to speak *as* the simulator — a crossing is a
decision a region makes, and a grid that simulates no movement has to be told
to make it — which
is what `TestContext::fake()` hands a case. It is `None` on every live grid, and
a case that reaches for it declares a fake grid and nothing else.

The `cargo test` run writes no record: there the assertion is the record,
re-made from scratch every run. `run-offline` runs the same cases through the
same code (`fake::run_case`) and does record them, under `records/fake-sl/` and
`records/fake-opensim/`, so the reporter can set each beside the live grid it
imitates.

### Which grid the fake one is

There are **two** fake grids, not one: `Grid::FakeSl` and `Grid::FakeOpensim`.
The fake grid can be either live grid where the two disagree
(`sl_fake_grid::ImitatedGrid`), and the flavour is *the grid* rather than a
setting on the run — `--grid fake-opensim` starts a different grid, and a case
declares the flavours it is meaningful on in `grids()` exactly the way it
declares it is meaningless on aditi.

That is deliberate, and the alternative was tried first: one fake grid with a
per-behaviour knob produces a grid that is nobody. Before `ImitatedGrid`, a
stock fake grid announced `platform: OpenSim`, kept every login field like
OpenSim, and withheld a taken object's asset like Second Life, all at once — and
a viewer passing against that has not been tested against anything.

**Every offline case names both flavours.** The fake grid exists to be each
live grid in turn, and a case run against one flavour only leaves the other
free to drift from what it imitates — which is how a grid that is nobody comes
back. The exceptions are listed in `fake::SINGLE_FLAVOUR`, each with its reason,
and a unit test holds every other offline case to both. There is one today:

- `asset-round-trip` names **`Grid::FakeOpensim` alone**, because reading a
  taken object's asset back is something only OpenSim ever lets a viewer do. It
  does not run on the Second Life flavour and claim to have tested it.

An exemption says the other flavour has *nothing to assert*, never that it
fails: a case failing on one flavour is a fake grid that does not yet imitate
its live grid, and the fix is the grid or a per-grid answer, below.

### Holding a flavour to its live grid

Where the two live grids answer differently, a case states both answers as a
`measured::Measured` constant, beside the code that observes it, naming where
the measurement is written down:

```rust
use sl_conformance::measured::Measured;

/// How each grid announces the item a take creates.
const TAKE_ANNOUNCEMENT: Measured<&str> = Measured {
    second_life: ANNOUNCED_BULK,
    opensim: ANNOUNCED_LEGACY,
    source: "object-rez-derez's take on aditi and OpenSim (2026-09-07)",
};

/// In the case body, where the take's announcement was `seen`.
fn hold(grid: Grid, seen: &str) -> Result<(), TestFailure> {
    TAKE_ANNOUNCEMENT.check("the announcement of a taken object", grid, &seen)
}
```

`check` picks the answer by `Grid::behaves_like`, so one line pairs all four
grids: a `FakeSl` run is held to the **aditi** answer and a `FakeOpensim` run to
the **OpenSim** one, and each live run to its own. A fake grid that drifts fails
offline, on every commit; a live grid that changes its behaviour (or a book
table that was wrong) fails the run that measured it, instead of leaving a stale
table behind. The failure quotes `source`, so whoever reads it knows which
table to re-check. `simulator-features` (`OpenSimExtras`, `VoiceServerType`)
and `object-asset-format` (the take announcement) are written this way;
`economy-data`, which compares a seventeen-field price list, keeps its own
table from `ImitatedGrid::prices`.

For the `gridspec-*` tasks the `source` is the book's
[Grid Behaviour](../gridspec/index.md) table the answer was written into, with
the case and date it was measured. A gridspec feature is done when:

1. it is **measured** on aditi and on OpenSim — a conformance case recording
   the whole shape, a `sl-repl --script` probe, the viewer automation, or the
   user driving a viewer for what none of those reach;
2. it is **documented** as a table per behaviour in `book/src/gridspec/`, a
   column per grid, each cell saying how and when it was measured;
3. the **fake grid** gives each flavour's answer — a row in `imitates.rs` per
   divergence — and the case runs offline on **both** flavours with the
   answers as `Measured` constants, so the fake grid is held to the
   measurement (a large gap becomes its own implementation task);
4. the **viewer** handles each grid's answer — an `e2e` test against each fake
   flavour where possible, a live check otherwise.

### Live and fake side by side

`sl-conformance-report` lays its columns out in pairs, each live grid followed
by the flavour imitating it (`Grid::REPORTED`: `opensim`, `fake-opensim`,
`aditi`, `fake-sl`). Under a case whose live grid and fake twin both have a
newest run it lists every recorded field the two answer differently:

```text
    [opensim ≠ fake-opensim] dwell            7.611 (live) vs 42.500 (fake)
```

That listing is not a verdict. Some fields legitimately differ — a parcel's
name describes the fixture, not the grid — and the ones that must not are held
by a `Measured` check that fails the case. What it shows is the *unasserted*
rest, which is where the next measurement starts. Timings are left out (a
duration measures the machine as much as the grid), and so are ids on both
sides, since every grid mints its own.

What the flavour decides — a taken object's asset, the login response's
`options` handling, the price list, and six more — is [audited in the fake
grid's own docs](../tools/fake-grid.md#which-grid-the-fake-one-is).

## The aditi cooldown

aditi rate-limits per account, so the runner keeps a per-avatar login cooldown
(`sl_repl::LoginCooldown`) under the user's state directory,
`$XDG_STATE_HOME/sl-client/login-cooldown/<avatar>.timestamp`. Before an aditi
login, if the same avatar logged in within the last two minutes, the run is
refused (naming the seconds remaining) unless you pass `--force`. The local
OpenSim grid has no cooldown. A two-account test guards each avatar
independently.

The stamps are shared: the end-to-end stage (`sl-e2e`, `SL_E2E_GRID=aditi`)
reads and writes the same ones — it waits the window out rather than refusing —
and so does every worktree of the workspace, so no harness can log an avatar in
right after another one did.

## Case isolation: panics, hangs, and grid state

The case body runs isolated (`src/isolate.rs`), because everything after it —
the logouts and the record write — matters even when it goes wrong:

- a **panic** in the body is caught and becomes a `TestFailure::Panic`. Without
  that the process would unwind past the logout, leaving the avatar logged in on
  the grid so the next run's login has to evict a ghost presence;
- a **hung** body is cancelled at an overall timeout and becomes a
  `TestFailure::Timeout`. The default is generous (15 minutes — a backstop
  against an unbounded wait, not a performance assertion); a case overrides it
  with `GridTest::timeout`, and `--timeout <secs>` overrides both.

Either way the run is recorded as a failure and the avatars are logged out. The
failure reason is printed on the `FAIL:` line and written to the log, not into
the record — records are committed and a message can quote grid content.

A case that **mutates grid state** must restore it on the failure path too, not
only at the end of a happy flow. `parcel-divide-join` is the worked example: its
divide leaves the region genuinely split, so the exercise runs under an awaited
join that covers every path that returns, plus a `Drop` guard that queues the
same join (via `Session::commander()`, since `Drop` cannot await) for the paths
that never return — a cancelled body or an unwind. Cases that create *grid-side*
resources they cannot delete (a group created by `support::membership_group`
after a retry) name the leftovers in the log and in an `orphan_group_count`
metric instead.

## Adding a test

A test is a `GridTest` (see `src/registry.rs`) registered in `registry()`:

```rust
impl GridTest for MyTest {
    fn name(&self) -> &'static str {
        "my-test"
    }
    fn description(&self) -> &'static str {
        "What it checks"
    }
    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }
    fn accounts(&self) -> u8 {
        1
    }
    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            ctx.primary()
                .wait_for_region(Duration::from_secs(60))
                .await?;
            // drive the session, record metrics, return Ok(()) or a TestFailure
            Ok(())
        })
    }
}
```

The body receives a `TestContext` whose `primary()` (and, for two-account tests,
`secondary()`) sessions are already logged in. Drive them with `send` and
`wait_for`, and record measurements via `ctx.metrics()`:

- `set(key, value)` — a neutral count.
- `set_timing(key, seconds)` — a duration, marked "lower is better" so the
  reporter colours its trend.
- `set_partial(key, value)` — a value covering only part of the dataset.

If the run truncates or aborts but still records useful numbers, call
`ctx.mark_partial("reason")` so the reporter never compares those counts against
a complete run's.

Restrict `grids()` to the grids where the feature exists — e.g. an
experiences-only test returns `&[Grid::Aditi]`, and the reporter shows `n/a` for
OpenSim. Adding the fake grid means declaring **both** flavours (or listing the
case in `fake::SINGLE_FLAVOUR` with its reason), adding the name to
`fake::OFFLINE_CASES`, and adding a line
`case => "case" [fake_sl, fake_opensim],` to `tests/offline.rs`; unit tests fail
if any of the three disagree.
