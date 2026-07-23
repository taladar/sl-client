---
id: viewer-help-onboarding-misc
title: Onboarding/help odds-and-ends
topic: viewer
status: ideas
origin: main-menu survey (2026-07-23)
refs: [viewer-report-abuse, viewer-login-screen, viewer-notification-history]
---

Context: [context/viewer.md](../context/viewer.md).

A grab bag of small Help/Content-menu features with no coverage, each
needing individual scoping before promotion:

- **Viewer UI hints** (`ToggleUIHints`) + **Guidebook** floater
  (`Help.ToggleHowTo`): onboarding hint bubbles and the how-to guide.
- **Whitelist adviser** (`fs_whitelist_floater`): FS's
  antivirus-exclusion adviser — likely minimal relevance on Linux;
  evaluate before adopting.
- **Report Problem / Bug** flow (`Advanced.ReportBug`) — distinct from
  abuse reporting ([[viewer-report-abuse]]); on FS this opens the JIRA
  flow, for us likely a GitHub-issue link with prefilled sysinfo.
- **Sysinfo button in IM** (`SysinfoButtonInIM`): paste system info into
  a support conversation.
- **MOTD overlay toggle** (`Advanced.ToggleHUDInfo motd`): show the
  grid message-of-the-day on screen after login
  ([[viewer-login-screen]] shows it at login only).

Reference (Firestorm, read-only): `menu_viewer.xml` Help/Content
sections, `menu_login.xml`.
