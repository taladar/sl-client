---
id: viewer-automation-testkit-locators
title: The in-process tiers speak locators too
topic: viewer
status: blocked
origin: viewer automation design (2026-09-28)
points: 3
blocked_by: [viewer-automation-locator-engine, viewer-automation-app-builder]
refs: [viewer-ui-interaction-harness, viewer-world-test-harness]
---

Context: [context/automation.md](../context/automation.md).

`InteractionTest`, `WorldTest` and `ViewerHarness` address widgets by `Name`
string (`click_node`, `text_of`, `centre_of`). Once the semantic model and
locator engine exist, the cheap tiers can use the same vocabulary as the
end-to-end tier, so a test moves down a tier without being rewritten.

## Wanted

- Testkit helpers that resolve a `Locator` in an `&mut App` (`locate`,
  `click_locator`, `expect_disabled`, …) through the same engine, with the
  same actionability checks.
- `ViewerHarness` (now built by `ViewerAppBuilder`, with its UI groups)
  gains the same helpers.
- Convert a handful of existing `Name`-string tests as the worked example,
  including one that asserts a disabled button does nothing.

Acceptance: the converted tests pass; an actionability failure in the
testkit reads the same as one in the end-to-end tier.
