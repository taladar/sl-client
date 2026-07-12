---
id: test-setup-cost-appendix-open
title: Setup-cost appendix (OpenSim)
topic: test
status: done
origin: TEST_ROADMAP.md — Setup-cost appendix (OpenSim)
---

Context: [context/test.md](../context/test.md).

What must be enabled before a feature can be live-tested locally. Each points at
a memory note with the full procedure.

| Area / phases | Default | To enable | Memory note |
| --- | --- | --- | --- |
| Movement / physics | ubODE off | set `physics = ubODE` in OpenSim.ini | `opensim-needs-real-physics-for-movement` |
| Profiles, picks (7) | off / wrong URL | enable UserProfilesService, fix ProfileServiceURL to :9000 | `sl-client-opensim-profiles-setup` |
| Scripting (9), scripted objects (8) | XEngine off | enable XEngine, load a scripted OAR, restart, match region | `sl-client-opensim-scripted-object-testing` |
| Groups (3, 6) | Groups V2 off | podman MariaDB 10.6 on :3307, MessageOnlineUsersOnly + GridUser config | `sl-client-opensim-groups-v2-setup` |
| Money (15) | off | enable BetaGridLikeMoneyModule (balance hardcoded 0, transfers need a real backend) | `sl-client-opensim-money-module-setup` |

General live-test setup (start `opensim.service`, test avatars, console output
in the journal): memory `sl-client-test-avatar-and-smoke-tests`. Aditi login
(rustls, `credentials.aditi.toml`, YubiKey TOTP): memory
`sl-client-aditi-live-testing`.
