# The Runner

`sl-conformance` runs exactly one test against one grid per invocation.

## Commands

```text
sl-conformance run    --grid <opensim|aditi|fake-sl|fake-opensim>
                      [--avatar <name>] [--secondary <name>]
                      [--credentials <path>]
                      [--fixtures <path>] [--force] [--timeout <secs>]
                      <TEST>
sl-conformance list   [--grid <opensim|aditi|fake-sl|fake-opensim>]
sl-conformance generate-manpage --output-dir <dir>
sl-conformance generate-shell-completion --output-file <f> --shell <shell>
```

- `run` takes a single positional `TEST`. There is no batch form: running tests
  one at a time is the primary safeguard against aditi rate-limiting.
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
`fake::OFFLINE_CASES`, each on its own fresh grid. Those cases are therefore
exercised on **every** `cargo test` — and so on every commit — instead of the
next time somebody remembers to log a live grid in. A unit test pins the list
against the registry in both directions, so a case cannot declare *a* fake grid
without being run, nor be listed without declaring one.

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

Nothing offline writes a record. The committed `records/` tree holds the last
known answer from a grid somebody had to log into; this answer is re-made from
scratch every run, so a stored copy could only ever be staler than the truth —
which is why `Grid::RECORDED`, the reporter's default column set, is the two
live grids.

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

Almost every case names only `Grid::FakeSl`, which is what this workspace
targets and what the fake grid is by default. Two do otherwise, and they are the
two shapes worth copying:

- `asset-round-trip` names **`Grid::FakeOpensim` alone**, because reading a
  taken object's asset back is something only OpenSim ever lets a viewer do. It
  does not run on the Second Life flavour and claim to have tested it.
- `object-asset-format` names **both**, because it is a survey of exactly the
  thing the two disagree about: `take_step` reads `item-created-nil-asset` on
  one and `item-created-with-asset` on the other, and both answers are worth
  having. `run_offline_case` runs such a case once per flavour, under its one
  name, and a failure says which flavour failed.
- `economy-data` names both for a related reason: the two grids' price lists
  differ in twelve of seventeen fields, so the case compares the whole reply
  against the flavour's own list. That is what makes "a fake grid quotes the
  grid it says it is" a test rather than a claim in a doc comment.

What the flavour decides — a taken object's asset, the login response's
`options` handling, the price list, and six more — is [audited in the fake
grid's own docs](../tools/fake-grid.md#which-grid-the-fake-one-is).

## The aditi cooldown

aditi rate-limits per account, so the runner keeps a per-avatar login cooldown
under the gitignored `.sl-conformance/aditi-last-login/<avatar>.timestamp`.
Before an aditi login, if the same avatar logged in within the last two minutes,
the run is refused (naming the seconds remaining) unless you pass `--force`. The
local OpenSim grid has no cooldown. A two-account test guards each avatar
independently.

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
OpenSim. Adding a fake grid also means adding the case's name to
`fake::OFFLINE_CASES` and a line to `tests/offline.rs`; a unit test fails if you
do one without the other.
