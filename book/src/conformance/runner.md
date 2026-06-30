# The Runner

`sl-conformance` runs exactly one test against one grid per invocation.

## Commands

```text
sl-conformance run    --grid <opensim|aditi> [--avatar <name>]
                      [--secondary <name>] [--credentials <path>]
                      [--fixtures <path>] [--force] <TEST>
sl-conformance list   [--grid <opensim|aditi>]
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

## The aditi cooldown

aditi rate-limits per account, so the runner keeps a per-avatar login cooldown
under the gitignored `.sl-conformance/aditi-last-login/<avatar>.timestamp`.
Before an aditi login, if the same avatar logged in within the last two minutes,
the run is refused (naming the seconds remaining) unless you pass `--force`. The
local OpenSim grid has no cooldown. A two-account test guards each avatar
independently.

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
OpenSim.
