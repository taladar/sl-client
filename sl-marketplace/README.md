# sl-marketplace

Sans-I/O model of the Second Life Marketplace (SLM) DirectDelivery JSON
REST API: typed listing, merchant-status, and error records plus request
builders and response parsers for every route the reference viewer
drives. The crate performs no I/O — a runtime pairs each built
[`Request`](crate::Request) with an HTTP client of its choice and feeds
the status code and body text back into the parsers.

Unlike every other transport in the Second Life protocol surface (LLUDP
datagrams, LLSD-over-HTTP capabilities), the SLM API is plain JSON over
the region's `DirectDelivery` capability URL, which is why it lives in
its own crate rather than in `sl-wire`.
