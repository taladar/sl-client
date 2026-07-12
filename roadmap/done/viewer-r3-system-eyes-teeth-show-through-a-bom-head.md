---
id: viewer-r3
title: System eyes/teeth show through a BoM head
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R3. System eyes/teeth show through a BoM head.** Fixed by the R1
**weight-normalization** fix (confirmed live: the mesh head's teeth, eyes, and
eyelids now render cleanly). The "show through" was **misdiagnosed** as a
hiding gap: those parts are the *worn mesh head's own* rigged eyes / eyelids /
teeth, which had the R1 un-normalized-weight streak and protruded through the
mesh face — not the system `avatar_head.llm` parts poking out. Renormalizing
the skin weights seats them back inside the head. (The only remaining eye gap
is a missing eye *texture*, a fetch/material matter, not geometry — out of
scope here.) Note: this is distinct from **R2**, the *rigid* system eyeballs
(`avatar_eye.llm`), which are unaffected by the skinning fix and stay open.
