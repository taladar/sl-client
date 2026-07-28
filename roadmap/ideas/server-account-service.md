---
id: server-account-service
title: Accounts and identity service
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
---

Context: [context/server.md](../context/server.md).

User identity for the whole grid: account records (agent id, first/last
name, email), credential storage (password hashing, MFA/TOTP secrets),
account lifecycle (create/suspend/delete), god/admin levels, and the
adjacent per-user grid state OpenSim splits into `GridUserService` —
home location, last location, online times.

Also the natural owner of **display names** (the `GetDisplayNames` cap
backend and the update flow with its cooldown rules) and of avatar
**profiles/picks/classifieds** storage (OpenSim: UserProfilesService),
unless profiles get their own service.

Consulted by the login service for authentication and by simulators for
name resolution (`UUIDNameRequest`/`AvatarNamesRequested` backends).
