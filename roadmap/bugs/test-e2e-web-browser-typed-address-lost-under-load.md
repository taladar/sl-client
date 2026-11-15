---
id: test-e2e-web-browser-typed-address-lost-under-load
title: e2e_preferences' web browser test sees the home page, not the typed address
topic: test
status: bugs
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
