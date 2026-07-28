---
id: viewer-web-openid-auth
title: Second Life website auto-login (OpenID cookie) in the in-viewer browser
topic: viewer
status: done
origin: user request (2026-07-22), while shipping the CEF web-media engine
refs:
  [viewer-media-prim-browser, viewer-profile-web-tab-browser,
  viewer-search-floater]
---

Context: [context/viewer.md](../context/viewer.md).

**Done 2026-07-28.** The whole chain landed:

- **`sl-wire`** extracts `openid_url` / `openid_token` from the login response
  (optional; absent on OpenSim) into `LoginSuccess`, round-tripped by
  `build_login_response`. Threaded through the `sl-proto` `Session`
  (`openid_url()` / `openid_token()` accessors) into `SlIdentity`.
- **`sl-media` / `sl-cef`**: a `SharedCookie` POD and
  `MediaBackend::set_shared_cookie` / `clear_shared_cookies` (default no-ops).
  The CEF impl targets the **global** cookie manager — the store non-isolated
  (`isolated: false`) browser panels use — via
  `cookie_manager_get_global_manager`; the per-in-world isolated contexts are
  never touched.
- **Viewer** (`src/web_auth.rs`, new): at login, when `SlIdentity` carries the
  OpenID pair, POST the raw token to `openid_url`
  (`application/x-www-form-urlencoded`, no redirect-follow) on a worker thread,
  parse the reply's `Set-Cookie` into a `SharedCookie` scoped to the OpenID
  host (URL == domain, per reference MAINT-5711), and inject it into the shared
  context so the web floater / profile Web tab / search Web tab open signed in
  (SSO against the OpenID host). `--no-web-auth` (or a disabled web engine)
  skips it. The injected cookie is a session cookie and CEF keeps
  `persist_session_cookies` off, so it is memory-only; it is additionally
  cleared **on clean exit** (the common case) and **before each login injects**
  (authoritative — self-heals a crash-orphaned session, which writes no
  `AppExit`, and covers a future in-process avatar switch).
- **Search Web tab** now builds the reference's full templated SL URL
  (`search.secondlife.com/viewer/?query_term=…&search_type=standard`
  `[collections]&maturity=…&lang=…&sid=[SESSION_ID]`, `build_sl_search_url`)
  on Second Life, keeping the OpenSim `search-server-url` query-substitution
  path. This closes the [[viewer-search-floater]] "full SL templated web URL"
  follow-up.

**Deliberate simplifications / deferred:** the reference additionally GETs the
web-profile URL to prime the snapshot-upload auth cookie
(`LLWebProfile::setAuthCookie`) — out of scope until photo hosting
([[viewer-photo-hosting-upload]]); embedded browsers mint the site session on
first navigation via the injected OpenID cookie, so no priming GET is needed
for the browse-logged-in win. The shared CEF cache is still one dir for all
avatars (a per-account `cache_path` would need CEF init deferred past login,
which breaks the eventual login-page-in-browser use); the on-exit clear covers
the single-session process today. Testable only against real Second Life
(aditi): OpenSim sends no `openid_url`.

**Verification (2026-07-28):** unit tests cover the wire parse/serialize
(present + absent), the `Set-Cookie` parse and the templated search URL. A live
aditi run confirmed the CEF engine initialises and the flow correctly logs
"login response carried no OpenID token; web surfaces browse anonymously" — i.e.
**aditi does not issue an OpenID token at all** (only agni does), so the actual
cookie injection producing a signed-in browser is verifiable only on agni,
which was intentionally not exercised yet. The aditi Web search still shows a
site login button for exactly this reason (its account has no production web
session).

The reference viewer logs the **grid account into the Second Life websites**
(my.secondlife.com web profiles, marketplace, search) automatically, so the
in-viewer browser opens them already authenticated. Our web floater and
profile Web tab currently browse anonymously.

**Also covers the Search floater's Web tab** ([[viewer-search-floater]]): its
embedded browser is on the same shared (trusted-UI) request context, so once
this auto-login lands the Web search opens **logged in** for free — and the
same login response supplies the `search_token` the full SL templated search
URL wants (`search.[GRID]/viewer/?...&sid=[SESSION_ID]`), which the search
floater currently substitutes only the query into a base URL. Fold that
templated-URL build (per-grid host + session token + maturity/collection
params) into this task rather than the search floater.

## How the reference does it (verified in Firestorm source)

1. The **XML-RPC login response** carries two extra members: `openid_url`
   and `openid_token` (`llstartup.cpp:5170`, grid-side; OpenSim does not
   send them).
2. At login, `LLViewerMedia::openIDSetup` POSTs the raw token to
   `openid_url` (`Content-Type: application/x-www-form-urlencoded`) and
   keeps the reply's `Set-Cookie` header — the grid session cookie
   (`llviewermedia.cpp` `openIDSetupCoro`).
3. `setOpenIDCookie` then (a) injects that cookie into the media browser's
   cookie store for the OpenID host (`getOpenIDCookie` → CEF `setCookie` /
   `storeOpenIDCookie`), and (b) GETs the web-profile URL once through the
   viewer's own HTTP stack so the redirect chain mints the site session;
   embedded browsers thereafter open the sites logged in. The web-profile
   panel relies on this (Firestorm additionally re-injects before
   navigating, and has had **recent regressions** in this area when the
   websites changed their cookie/redirect behaviour — worth checking their
   tracker for the current state before porting).

## What we need

- **`sl-wire`**: extract `openid_url` / `openid_token` from the login
  response (optional fields; absent on OpenSim).
- **`sl-cef`**: a cookie-injection call on the *shared* (trusted-UI)
  request context — CEF's `CookieManager::SetCookie` for a given URL, name,
  value, domain, path, secure, http-only. Never for the isolated in-world
  contexts: a griefer's media prim must not see the session.
- **Viewer**: at login, run the token POST off-thread, parse the
  `Set-Cookie`, inject it, and prime the profile-URL redirect chain; the
  web floater / profile Web tab then get it for free. Gate on the fields
  being present so OpenSim logins are unaffected.
- Consider a `--no-web-auth` escape hatch, and make sure logout / avatar
  switch clears the shared cookie store (`sl-account-dirs` scoping: the
  shared context currently persists under one cache dir for all avatars —
  per-avatar separation likely wants a per-account `cache_path`).

Testable only against real Second Life (aditi): OpenSim sends no
`openid_url`. Reference (read-only): `llviewermedia.cpp`
(`openIDSetup[Coro]`, `openIDCookieResponse`, `setOpenIDCookie`,
`getOpenIDCookie`, `parseRawCookie`), `llstartup.cpp` (login response),
`llpanelprofile.cpp` (web tab consuming it).
