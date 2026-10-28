---
id: viewer-lsl-syntax-cache-never-enabled
title: The viewer never caches the grid's LSLSyntax document on disk
topic: viewer
status: done
origin: server-fake-grid-lsl-syntax (2026-10-28)
refs: [protocol-lsl-syntax, server-fake-grid-lsl-syntax]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`sl-client-common::lsl_syntax_cache::LslSyntaxCache` keeps a fetched
`LSLSyntax` document by its id under the shared cache directory, so a
restart on the same grid does not fetch the few hundred KiB again (Firestorm
does the same, `llsyntaxid.cpp`). The viewer never turns it on: it builds its
client with `directories: ClientDirectories::default()`
(`sl-client-bevy-viewer/src/assembly.rs`), whose `shared_cache_dir` is
`None`, and `sl-client-bevy` hands that `None` to the cache. A headless run
against the fake grid on 2026-10-28 fetched and decoded the document and left
no `lsl-syntax/` directory anywhere under its `XDG_CACHE_HOME`.

`sl-conformance`, `sl-repl-tokio` and `sl-repl-bevy` set the directory; the
viewer is the one client that does not.

## Done when

The viewer passes a shared cache directory (beside its other caches under the
viewer's cache root), a second login to the same grid loads the document from
disk without a fetch, and a test or a logged line shows which happened.

## Fixed (2026-10-28, with server-fake-grid-lsl-syntax)

The viewer passes `ViewerPaths::resolved_cache_root()` as the client's
`shared_cache_dir` (`sl-client-bevy-viewer/src/assembly.rs`), so the document
lands in `<cache root>/lsl-syntax/` and follows a `CacheLocation` override;
Clear Cache deletes it with the other regenerable caches (`PURGE_KIND_DIRS`).
Both runtimes now log which way a document came: `fetching the grid LSL
syntax definition id=…` or `grid LSL syntax definition from the disk cache
id=…`. Two headless logins to the fake grid in one cache directory logged the
fetch, then the cache hit, under the same id.
