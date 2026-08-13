# Marketplace JSON (SLM)

Every transport described so far speaks one of two encodings: binary
LLUDP datagrams, or LLSD over HTTPS capabilities. The Second Life
Marketplace (SLM) listing-management service is the protocol surface's
one exception — a **plain-JSON REST API** reached through the region's
`DirectDelivery` capability. It manages the resident's marketplace
*listings* (the folders under the Marketplace Listings special folder
that back products on the marketplace website); the website itself and
the purchase flow remain out of protocol scope. Second Life only:
OpenSim grids do not serve the capability at all.

## Capability discovery

`DirectDelivery` is requested from the region seed like any other
capability. Its URL is not a single endpoint but the **base URL** of
the SLM service: each route below is appended to it verbatim (the
reference viewer's `getSLMConnectURL` simply concatenates), so the
grid controls whatever path prefix the service lives under. A region
that does not grant the capability leaves the client unable to reach
the marketplace — the reference viewer reports that as a connection
failure rather than an error from the service.

## The route table

| Method | Route | Body | Reply |
|--------|-------|------|-------|
| GET | `/merchant` | — | status code only |
| GET | `/listings` | — | listings envelope |
| POST | `/listings` | create payload | listings envelope |
| GET | `/listing/<id>` | — | listings envelope (404 = gone) |
| PUT | `/listing/<id>` | update payload | listings envelope |
| PUT | `/associate_inventory/<id>` | associate payload | listings envelope |
| DELETE | `/listing/<id>` | — | envelope of deleted ids |

`<id>` is the listing's numeric id rendered as a plain decimal path
segment. Every route except the merchant probe sends both
`Accept: application/json` and `Content-Type: application/json`; the
probe sends neither header (reference-viewer parity). There are no
query parameters and no pagination — `GET /listings` returns the whole
set in one reply, and lookup-by-folder is a client-side join against
the locally cached listings.

## The merchant probe

`GET /merchant` is unusual in that its payload is the **HTTP status
code itself** — the reply body is never parsed on success:

- any **2xx** — the agent is a marketplace merchant;
- **404** — the agent is not a merchant;
- **503** — the agent is a merchant whose store has not been migrated
  to DirectDelivery;
- anything else — a connection failure (the body is consulted only as
  a best-effort source for an `error_code` reason string).

## JSON framing

Every other route answers with the same envelope — a `listings` array,
even when the route addresses a single listing:

```text
{
  "listings": [
    {
      "id": 12345,
      "is_listed": true,
      "edit_url": "https://marketplace.secondlife.com/p/…/edit",
      "inventory_info": {
        "listing_folder_id": "11112222-3333-4444-5555-666677778888",
        "version_folder_id": "00000000-0000-0000-0000-000000000000",
        "count_on_hand": 3
      }
    }
  ]
}
```

Ids and stock counts are JSON integers, the listed flag is a JSON
boolean, and inventory folder keys travel as hyphenated UUID strings —
the null UUID (all zeros) marks a listing with no version folder
picked yet. Request bodies wrap their payload in a `listing` object:

```text
POST /listings
{"listing": {"name": "My Product",
             "inventory_info": {"listing_folder_id": "…",
                                "version_folder_id": "…",
                                "count_on_hand": 0}}}

PUT /listing/12345
{"listing": {"id": 12345, "is_listed": true,
             "inventory_info": {"listing_folder_id": "…",
                                "version_folder_id": "…",
                                "count_on_hand": 3}}}

PUT /associate_inventory/12345
{"listing": {"id": 12345,
             "inventory_info": {"listing_folder_id": "…",
                                "version_folder_id": "…"}}}
```

The associate form carries no stock count (the service recomputes it)
and no listed flag. All listing edits — listing/unlisting, switching
the version folder, updating the stock count — are the same
`PUT /listing/<id>` with different field values; the reference viewer
unlists a listing whenever its version folder is cleared. `DELETE`
archives a listing; its reply's array elements only guarantee the
`id` field.

## Error replies

Error bodies are looser than the success envelope, and the reference
viewer handles three shapes:

- a JSON **object** carrying `error_code` / `error_description`;
- a JSON **array** (or bare scalar) of message strings — with one
  special case: HTTP **422** with more than four messages means "the
  listing is incomplete and cannot be listed" (no version folder,
  empty stock, …);
- **5xx** replies, whose bodies are deliberately not parsed.

One status is *not* an error: `GET /listing/<id>` answering **404**
means the listing no longer exists on the marketplace, and a client
mirroring listings should drop its local record.

---

> **In this codebase**
>
> - The sans-I/O API model is the `sl-marketplace` crate: typed records
>   (`Listing`, `ListingId`, `InventoryInfo`, `MerchantStatus`,
>   `ApiError`), a request builder per route
>   (`merchant_status_request`, `listings_request`, …, each yielding a
>   `Request` with the verbatim path, optional pre-serialized body, and
>   the JSON-headers flag), and the response parsers
>   (`parse_merchant_status`, `parse_listings_response`,
>   `parse_deleted_ids`, `parse_error_body`).
> - `sl-proto` requests the capability (`CAP_DIRECT_DELIVERY` in
>   `REQUESTED_CAPABILITIES`), defines the seven `Command::Marketplace*`
>   variants and their `Event::Marketplace*` replies, and centralizes
>   the status+body→event mapping in `sl-proto/src/marketplace.rs`
>   (`marketplace_reply_event` / `marketplace_failure_event`) so both
>   runtimes agree on it.
> - Because the replies are JSON, they do not ride the LLSD
>   `handle_caps_event` path: the runtime helpers
>   (`sl-client-tokio/src/marketplace.rs`,
>   `sl-client-bevy/src/marketplace.rs`) perform the HTTP round-trip
>   and send the fully-formed event directly — the same pattern as the
>   experience-capability fetchers. A missing capability (e.g. on
>   OpenSim) surfaces as a connection-failure / transport event rather
>   than a silent drop.
> - The `marketplace-direct-delivery` conformance case (aditi-only)
>   records the transport ceiling reachable without a merchant store:
>   the probe's proper non-merchant answer and the `GET /listings`
>   behaviour.
