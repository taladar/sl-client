# Parallel work plan (drafted 2026-09-17)

How to split the remaining open tasks across several agents in separate
worktrees, and what to do **before** splitting. Written when the bug list was
nearly empty (6 open bugs, 456 open tasks: 315 ready, 41 blocked, 100 ideas) and
parallel feature work was about to resume.

Working style this is written for: each agent builds a branch of roughly **5–10
commits** in its own worktree, which is merged soon rather than kept alive for
weeks. Short branches make an occasional conflict in a shared file cheap, so the
split below optimises for *mostly* disjoint files, not perfectly disjoint ones.

## Why `topic:` cannot be used to split the work

404 of the 456 open tasks carry `topic: viewer`. The useful signal is the task
id prefix plus the title, and behind that **which crate the work lands in**. The
split below is by crate ownership.

## Phase 1 — the overarching rewrites, one at a time, before any split

These rewrite files every agent touches, and they are not 5–10 small commits, so
running one during a parallel round invalidates the other branches wholesale.
They also *create* the separation the later rounds rely on.

1. ~~`viewer-audit-world-api-split`~~ — **done**. `sl-viewer-world-api` is now
   nine modules, and the social / intent half has left it for the new
   `sl-viewer-social` and `sl-viewer-intents` crates. Each agent edits its own
   module instead of queueing on one file.
2. ~~`viewer-audit-notifications-crate-split`~~ — **done**. The ~21,600-line
   file is now `lib.rs` (types, lookup, state, tests), `forms.rs` (the button
   tables) and `catalogue/<family>.rs` x 31. A notification task edits its own
   family's file, and `NOTIFICATIONS` is unchanged for every caller.
3. ~~`viewer-audit-binary-module-extraction`~~ — **done**. 14,171 lines left
   `sl-client-bevy-viewer` (47,760 → 33,589) for six existing feature crates
   plus two new ones, `sl-viewer-ui-context-menus` (the four pie-menu entry
   trees) and `sl-viewer-gallery` (both offline galleries, which now take the
   element and floater registries as arguments rather than reaching into the
   binary). What is left in the binary is the composition root, the harness
   tiers, and the seven surfaces that genuinely belong to it — so the world
   agent and the UI agent no longer share a 19k-line `src/`.
4. ~~`protocol-audit-runtime-shared-crate`~~ — **done**. The four modules that
   were copies (`chat_log`, `inventory_cache`, `lsl_syntax_cache`, `retry` —
   1,782 lines) now live once in the new `sl-client-common`, which both
   runtimes depend on. A fix to any of them is one edit again, and the 18 tests
   over them run once. What is still per-runtime is `http_proxy.rs` and
   everything owning a task, a channel or a timer.

Items 1–3 are large mechanical rewrites with little behavioural change: verify
them with a normalised token diff against `git show HEAD:<file>` (see the
`sl-client-verify-mechanical-refactor` memory), and put a `sl-crosscheck` run
either side of anything that reaches the renderer.

## Phase 2 — small sweeps worth doing in the same solo pass

Each is cheap, and each removes a conflict class the parallel rounds would
otherwise keep hitting.

- ~~`viewer-audit-plugins-own-their-schedule`~~ — **done**. A crate owning its
  own plugin means feature work stops editing the binary's wiring, which was the
  main world-agent / UI-agent collision.
- ~~`idiomatic-audit-bevy-system-param-bundles`~~ (128+ sites) and
  ~~`idiomatic-audit-dead-forward-api`~~ (10 crates) — **done**. Mechanical but
  workspace-wide: miserable as merge fodder, easy as a solo sweep.
- ~~`viewer-audit-ui-spawn-helper-consolidation`~~ and
  ~~`viewer-audit-table-sort-consolidation`~~ — **done**. The forty-odd copied
  spawn helpers are one `ui_spawn` module and the eleven copies of the sort
  comparator are one `ui_table::order_by_sort_keys`, so the UI agent's ~130
  tasks no longer multiply them.
- ~~`viewer-audit-kit-single-consumer-split`~~ — **done**. The eight modules
  with exactly one consumer crate (4,230 lines, 36% of `sl-viewer-kit`) now live
  in the crate that calls them, so a radar, shadow-cull or map-math edit stops
  rebuilding the twenty-odd crates stacked above the kit to reach one caller.
  What is left there is shared vocabulary.
- ~~`viewer-audit-ui-core-sound-coupling`~~ — **done**, and it was not three
  lines. `ui_sounds` is its own crate, but that on its own moved nothing:
  `sl-viewer-settings` carried `sl-client-bevy` (and the whole protocol stack
  behind it) back under ui-core. Cutting its two runtime couplings as well took
  ui-core's dependency closure from 559 packages to 420, and left
  `sl-viewer-settings` — a floor nearly every crate stands on — naming no
  runtime at all. **This was the last item in this section**, so Phase 3 can
  start.

`build-audit-ci-pipeline` was listed here — "worth having before three branches
are in flight" — and is now **deferred**: on a workspace developed on one
machine at a dozen commits a day, a push-triggered build of 815k lines re-checks
what the `ggh` pre-commit hook already checked on the same tree. Fresh-checkout
and portability cover comes with release preparation.

## Phase 3 — the three-way split

| Agent | Themes (approx. task counts) | Owns |
| --- | --- | --- |
| **A — world & render** | render 33, perf 44, avatar/appearance 28, region/parcel/land 28, input 26, camera 9, snapshot 4 (~170) | `sl-viewer-world-scene`, `-world-objects`, `-world-avatar`, `-world-view`, `sl-texture`, `sl-material`, `sl-bake`, `sl-anim`, `sl-terrain`, `sl-viewer-environment`, `sl-viewer-places` |
| **B — UI shell & social** | UI shell 34, chat/IM/notices 27, inventory 22, people/groups 12, map/search 7, i18n 7, misc shell 19 (~128) | the viewer binary's UI (`menu_bar.rs`, `floaters.rs`, `status_bar.rs`, `bottom_toolbar.rs`), `sl-viewer-ui-*`, `-preferences`, `-settings`, `-chat`, `-notices`, `-notifications`, `-people`, `-social`, `-intents`, `-inventory`, `-map`, `-search` |
| **C — tools, protocol & server** | build/edit tools 42, server 21, test/conformance 18, audio/media/voice 12, scripts/LSL 11, protocol 8 (~152) | `sl-viewer-edit`, `sl-wire`, `sl-proto`, `sl-client-tokio`, `sl-client-common`, `sl-fake-grid`, `sl-conformance`, `sl-crosscheck`, `sl-repl*`, `sl-lsl*`, `sl-audio`, `sl-gst`, `sl-cef` |

Why these groupings rather than by feature area:

- `sl-viewer-world-scene` is shared by render, land **and** perf, and
  `sl-viewer-world-view` owns camera, input and screenshots. Splitting those
  across agents would guarantee collisions, so they stay in **A**.
- The UI-shell, chat, inventory and people tasks all fight over the same files.
  In one agent (**B**) that is ordinary sequential editing rather than merge
  conflict.
- `sl-viewer-edit` is a self-contained 15-file crate, and the build floater is
  its own corner of the binary, so the build/edit tools balance **C** without
  colliding with **B**.

The two crates the binary extraction created are not covered by the glob above:

- **`sl-viewer-ui-context-menus` belongs to B**, despite what its entries act
  on. It is four entry trees and their enable/disable rules; the actions
  themselves are messages other crates answer, so an avatar-menu task is a
  menu edit, not a world edit.
- **`sl-viewer-gallery` is a shared hub** like `sl-viewer-kit`: A judges render
  scenes in it, B judges UI elements in it. The registries it renders stayed in
  the binary, so ordinary feature work adds an entry there and never touches
  this crate — changing the gallery itself is a `claim --subsystem` moment.

### Rules for a parallel round

- **Assign perf tasks by crate, not by theme.** The perf tasks are spread over
  avatar, objects, scene, texture *and* `sl-viewer-ui-core`'s virtual list; the
  last belong to B.
- **i18n belongs to B** — every user-visible string plus the shared
  `locales/*.ftl` files collide with anything that adds UI text.
- **Shared hubs stay read-mostly**: `sl-viewer-kit` and (post-split)
  `sl-viewer-world-api`. Changing one is a `roadmap/coord.sh claim
  --subsystem` moment so the other agents see it.
- **Guaranteed textual conflicts**, none of them interesting: `roadmap/INDEX.md`
  (regenerate with `python3 roadmap/index.py`, never merge by hand),
  `Cargo.lock`, and the root `Cargo.toml` members list.
- The remaining single-crate audits fold into normal rounds by owner: `sl-proto`
  ones (`protocol-audit-session-god-object`, `-conversions-test-coverage`,
  `-dispatch-child-drift`, `-wire-error-contract`, `-extract-lludp-transport`,
  `-decoder-fuzz-harness`, `repl-audit-binary-duplication`,
  `test-audit-conformance-boilerplate`) to **C**;
  `viewer-audit-object-children-index`, `-decoded-texture-uploaders`,
  `-render-fixtures-crate`, `-scene-live-daycycle-fixture` to **A**;
  `viewer-audit-preferences-hub-decoupling`, `-menu-label-i18n`,
  `-skin-token-coverage`, `-restart-note`, `-demo-panels-in-release`,
  `-web-auth-preference`, `-search-map-edge` to **B**.
