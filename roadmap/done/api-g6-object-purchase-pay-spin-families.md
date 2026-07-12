---
id: api-g6
title: Object purchase, pay, spin, families
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G6 — Object purchase, pay, spin, families

`ObjectBuy` + `BuyObjectInventory`; `RequestPayPrice`/`PayPriceReply`;
`ObjectSpinStart`/`Update`/`Stop`; `RequestObjectPropertiesFamily`/
`ObjectPropertiesFamily` (condensed broadcast properties);
`ObjectDuplicateOnRay`, `RezRestoreToWorld`, `RezObjectFromNotecard`.
OpenSim-testable.

- [x] G6 object commerce, spin, and property families. New types
  `ObjectBuyItem`, `NotecardRez`, `RestoreItem` (in `types/editing.rs`) and
  `ObjectPropertiesFamily` (`object.rs`). Commands `BuyObject` (`ObjectBuy`,
  a `Vec<ObjectBuyItem>`), `BuyObjectInventory`, `RequestPayPrice`,
  `RequestObjectPropertiesFamily`, `SpinObjectStart`/`SpinObjectUpdate`/
  `SpinObjectStop` (`ObjectSpinStart`/`Update`/`Stop`), `DuplicateObjectsOnRay`
  (`ObjectDuplicateOnRay`), `RezRestoreToWorld` (UDPDeprecated, still wrapped),
  `RezObjectFromNotecard`; matching `Session` methods + `circuit.rs` `send_*`
  encoders. Events `PayPriceReply` (default price + quick-pay buttons) and
  `ObjectPropertiesFamily` (condensed owner/perms/sale summary) decoded in the
  dispatch path. Server: each inbound message surfaces as a same-named
  `ServerEvent`, and `SimSession` gains `send_pay_price_reply` /
  `send_object_properties_family`. Both runtimes + REPL (10 commands;
  `buy_object` takes `local_id:sale_type:sale_price` records, the two big rez
  commands take keyword fields) + format.rs event/command names. Tests: 5
  lifecycle (commerce encode, rez encode, PayPriceReply decode,
  ObjectPropertiesFamily decode) + 1 loopback round-trip + 4 REPL registry. New
  book chapter `content/object-commerce.md` (+ SUMMARY entry). **Scope note:**
  `DerezContainer` (Low 104) is the `Trusted` sim→viewer half of the asset-buy
  handshake and is not wrapped, following the G1/G4/G5 trusted-backend
  precedent. OpenSim-testable but not live-tested this session (loopback
  round-trip covers the wire path both directions). **NEXT = G7** (parcel
  join/divide/object-owners/pass/disable/info).
