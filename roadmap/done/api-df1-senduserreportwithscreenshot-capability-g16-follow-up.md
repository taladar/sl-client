---
id: api-df1
title: SendUserReportWithScreenshot capability (G16 follow-up)
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## DF1 — `SendUserReportWithScreenshot` capability (G16 follow-up)

Second Life's abuse-report floater prefers `SendUserReportWithScreenshot` over
plain `SendUserReport` when a snapshot is attached: it first uploads the
snapshot as an asset, then POSTs the same report body referencing the new
`screenshot_id`. G16 implemented only the no-screenshot path (plain
`SendUserReport` cap + the `UserReport` UDP message), because the screenshot
variant needs the HTTP asset-upload pipeline (`LLViewerAssetUpload` /
`NewFileAgentInventory`-style two-step uploader) wired into the report POST.
Once that plumbing is reused, add the cap
(`CAP_SEND_USER_REPORT_WITH_SCREENSHOT`), a `screenshot` argument/flow that
uploads then fills `AbuseReport::screenshot_id`, and route
`SendAbuseReportViaCaps` through it when a screenshot is present. SL-only
(OpenSim has no abuse-report cap at all). Cross-check against Firestorm
`llfloaterreporter.cpp` `sendReportViaCaps` / `LLARScreenShotUploader`.

- [x] DF1 abuse report with screenshot upload. The two-step CAPS uploader
  that gated this (`NewFileAgentInventory` / `UploadBakedTexture`'s
  `run_caps_upload` / `caps_upload_step`, present in both runtimes) is now
  reused for the report path. Added `CAP_SEND_USER_REPORT_WITH_SCREENSHOT` (in
  `REQUESTED_CAPABILITIES`); reshaped `Command::SendAbuseReportViaCaps` from a
  tuple variant into `{ report, screenshot: Option<Vec<u8>> }`. When a
  screenshot is supplied and the region offers the cap, both runtimes upload
  the snapshot
  over a fire-and-forget two-step helper (`run_report_screenshot_upload`) —
  filling a fresh `screenshot_id` (a v4 texture asset id) and POSTing the report
  body referencing it — else they fall back to the plain `SendUserReport` POST.
  REPL `send_abuse_report_caps` gains an optional `screenshot=<hex>` arg (new
  `Args::opt_bytes` helper) + a parse test; book `world.md` documents the
  screenshot path. SL-only (OpenSim has no abuse-report cap at all); the
  screenshot cap is a no-op when the seed omits it.
