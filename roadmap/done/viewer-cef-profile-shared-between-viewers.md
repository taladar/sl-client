---
id: viewer-cef-profile-shared-between-viewers
title: Two viewers could not both run web media (one shared CEF profile)
topic: viewer
status: done
origin: live test of viewer-streaming-audio (2026-09-27)
---

Context: [context/viewer.md](../context/viewer.md).

Every viewer process, and the gallery too, pointed CEF at the same profile
directory, `<cache root>/cef`. Chromium takes an exclusive lock on its
profile, so whichever process started second failed `cef::initialize`
(exit code 24) and ran with no web media at all. The only sign was one
warning in its log. It showed up when a gallery in another worktree was open
while a viewer was being live-tested: the viewer's media faces never started.

## Fix (2026-09-27)

`sl_viewer_platform::paths::claim_media_engine_profile` gives each process a
profile of its own. It claims the first of `cef/profile-0`, `cef/profile-1`,
… whose lock file no running process holds, and the claim is an OS file lock
held for the rest of the process, so a crash frees it too.

The directories are numbered slots rather than one per process id. A lone
viewer comes back to the same profile every run, so a page keeps its cookies
and logins, and directories do not pile up. If all 64 slots are held, web
media is disabled with a warning that says so, rather than two viewers
sharing one profile. The claim logic is unit-tested (`paths`).
