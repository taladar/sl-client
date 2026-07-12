---
id: missing-batch-2
title: generic message family
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

## Batch 2 — generic message family

`GenericMessage` (Low 261), `LargeGenericMessage` (Low 430),
`GenericStreamingMessage` (High 31): a method-name + params envelope used by
many features. Surfaced as `Event::GenericMessage(GenericMessage)` /
`Event::LargeGenericMessage(GenericMessage)` (the large variant shares the
`GenericMessage { method: String, invoice: InvoiceId, params: Vec<Vec<u8>> }`
domain struct — identical shape, larger per-param wire limit) and
`Event::GenericStreamingMessage(GenericStreamingMessage { method: u16, data:
Vec<u8> })`, leaving feature-specific parsing to consumers. The `Invoice`
correlation id is the new `InvoiceId` newtype (in `bookkeeping_ids.rs`);
parameter blobs stay raw bytes (lossless — they are usually but not always
UTF-8 strings). The feature-specific `emptymutelist` `GenericMessage` and the
GLTF-material-override `GenericStreamingMessage` (method `0x4175`) keep their
existing dedicated arms, matched ahead of the generic fallback.
