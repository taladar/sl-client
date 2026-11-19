---
id: test-e2e-web-browser-typed-address-lost-under-load
title: e2e_preferences' web browser test sees the home page, not the typed address
topic: test
status: done
origin: commit hook's full suite while landing server-lsl-mono-collision-model-under-lowered-limit (2026-11-11)
---

Context: [context/automation.md](../context/automation.md).

## Observation

`e2e_preferences::the_web_browser_and_the_search_web_tab_load_a_page`
failed once in the commit hook's full run (4977 tests) and passed when run on
its own straight after. The web browser window opened by the menu
(`floater:web-browser#new-0`) never showed the address typed into it
(`http://127.0.0.1:<port>/browser`, then Enter). For the whole 60 s wait its
document read the default home page instead:
`https://secondlife.com/?openid_identifier=…anonymous`, titled "Official Site |
Second Life …".

## Likely cause

A race between the window's home page navigation and the typed one: under load
the home page's load either lands after the typed navigation and replaces it,
or Enter reaches the field before the floater takes the address. Passing alone
and failing under load points at ordering, not at a wait that is too short,
since the home page did load.

It failed the same way again in the hook's full run while landing
test-lsl-content-suite (2026-11-13), test 1246 of 8441, the document reading
`https://secondlife.com/` for the whole wait.

A sibling test fails the same way:
`e2e_preferences::every_page_keeps_a_browser_window_of_its_own` failed in the
hook's full run while landing server-lsl-lib-ossl-land-writers (2026-11-15),
test 1094 of 7074, after 69 s. Its `floater:web-browser#new-0` never showed
`http://127.0.0.1:<port>/first` and read the secondlife.com home page
throughout. All nine tests of the file passed alone straight after, that one
in 27 s. So the race is in the window's navigation, not in one test's steps.

Also worth noting: the test reaches the public internet (secondlife.com)
through the home page. An offline run would fail this way every time.

## Wanted

Find which of the two orderings it is (a trace of the browser's navigations
with their sources), make a typed address always win over a home page load
still in flight, and keep the test off the network, for example by staging a
local home page.

## Findings

`sl-cef` now traces each surface's navigations at debug level (`RUST_LOG=
info,sl_cef=debug`): creation, loading state, each main-frame commit with
its transition type, aborts, popups, held and issued navigations and mouse
buttons. Runs throttled to 10-15% CPU showed two separate races:

- **The home page's own follow-up navigation.** secondlife.com commits, then
  starts its OpenID client-redirect hop. That hop was still in flight after
  the typed page had loaded: the trace shows the home page's navigation
  aborted only afterwards. The URL the failures reported
  (`?openid_identifier=…anonymous`) is that hop. So the old page's own
  script navigated the window again after the typed address.
- **A click before the page could take it.** In
  `every_page_keeps_a_browser_window_of_its_own`, the click on the `/opener`
  page went out 128 ms after its commit, before the new document had painted.
  Chromium drops input to such a document, so no popup was asked for and the
  test timed out waiting for the named window.

## Fix

- A surface remembers the address it was last asked for, until a page
  commits. Meanwhile a main-frame navigation to anything else is cancelled in
  `on_before_browse`, unless it is a user gesture or a server redirect. The
  old page's script cannot overtake a typed address any more.
- `SurfaceStatus::presented` says whether the main frame's newly arrived
  document has presented. A paint is no evidence of that: paint holding keeps
  the old page's frames coming, and a paint can reach CEF before the commit
  does, which left a static page busy for good when the commit reset the flag.
  Each surface instead follows DevTools page events: `Page.frameNavigated`
  for the main frame starts a document by its loader, and that loader's
  `firstContentfulPaint` lifecycle event presents it. Chromium drops input
  until then. `networkIdle` lets a page with nothing to paint go. The
  observer attaches after a surface's first navigation has begun, so its
  `Page.getFrameTree` names the document already there, and enabling
  lifecycle events replays the steps that document has taken. While the
  page has not presented, or the view has no surface yet, the browser view
  carries the new `Busy` marker. Automation reports it as the `busy`
  state (and the screen reader as busy), and a click waits for it through
  the new `ready` actionability check.
- `FSBrowserHomePage` (the reference's name; default still
  `https://secondlife.com/`) is the page a window opened without an address
  shows. Both browser tests set it to a page on their loopback server.
- The search window's Web tab opens on the grid's own search site when the
  grid has already named one, not on Second Life's. That keeps the search
  tests offline and is what an OpenSim resident expects.

## Verified

Throttled to 15% CPU (`systemd-run --user --scope -p CPUQuota=15%`), both
tests ran four rounds each on both backends and passed every time. Before
the change, `every_page_keeps_a_browser_window_of_its_own` failed four out of
four under the same throttle. The trace shows each click going out only after
its page's `presented=true`, including the click the test now makes on a
fresh window's first page. The whole `e2e_preferences` file passes
unthrottled. New unit tests cover the superseded-navigation rule and the
reading of page events and the frame tree.
