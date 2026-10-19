# Money

What each grid does with a viewer's money traffic: a balance request, a gift
to another resident, a payment to an object and the pay buttons it offers, and
a purchase of an object set for sale — and, last, the Marketplace's listing
service ([§ Marketplace](#marketplace)). The messages themselves are described
in [Economy & Money](../content/economy.md) and
[Object Commerce](../content/object-commerce.md); land sales and passes are in
[Land § Transactions](land.md#transactions), and the price list
(`EconomyData`) and upload fees in [Assets](assets.md).

Measured on 2026-10-18 on Second Life's beta grid (aditi) and the local
OpenSim standalone by the census `money`, two avatars a run, twice on each
grid: **A** pays and sells, **B** is paid and buys. On aditi both stand at
the fixtures' build location (a sandbox), on OpenSim at the Default Region's
centre. Every amount is L$ 1 or L$ 0 and the census ends by giving B back
what it came to be short, so a run costs the test accounts nothing. The census
holds both grids to these answers as `Measured` constants.

The local OpenSim runs the stock `BetaGridLikeMoneyModule`
(`[Economy] economymodule`), which keeps no balances and moves no money: it is
what an OpenSim grid without a money server does. A grid with a money server
(`Gloebit`, `DTL/NSL`) answers through that server instead and is not
measured here.

## Balance

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `MoneyBalanceRequest` answered | `MoneyBalanceReply`, success, the account's balance, nil transaction id, no `TransactionInfo` block, empty description | the same, with a balance of **0** | the same, with the balance the session keeps (debited by its upload and group fees) |
| told to the other avatar | nothing | nothing | — |

## Gifts

`MoneyTransferRequest` with transaction type `5001` (`TRANS_GIFT`) and the
census's own description.

| A or B gives | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| another avatar L$ 1 | payer and payee each get a `MoneyBalanceReply` with a fresh transaction id, the new balance and a `TransactionInfo` block (type `5001`, source, destination, amount, the sender's description as `ItemDescription`); payer's description "You paid *payee* L$1.", payee's "*payer* paid you L$1." | **nothing**, to either side: the transfer is dropped | nothing |
| another avatar L$ 0 | nothing, to either side | nothing | nothing |
| itself L$ 1 | two replies, both naming A as source and destination: "You paid *A* L$1." and "*A* paid you L$1." | nothing | nothing |
| an id that is nobody's | a reply that **failed**, the transaction block naming the id, "There has been a problem transferring L$. This error has been logged and reported." | nothing | nothing |
| more than it holds | a reply that failed, "Insufficient funds." | nothing | nothing |

Second Life writes the other side's name in the description itself; the
reference does not show that description when the `TransactionInfo` block is
present, but builds its own notice from the block (below).

## Paying an object

B asks A's cube for its pay buttons (`RequestPayPrice`) and pays it L$ 1
(`MoneyTransferRequest`, type `5008`, `TRANS_PAY_OBJECT`) — first a plain cube,
then the same cube with a script that calls
`llSetPayPrice(7, [3, 5, PAY_HIDE, PAY_DEFAULT])` and says each `money` event
to its owner.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| pay buttons of a cube with no script | `PayPriceReply`: default `-2` (`PAY_DEFAULT`), buttons `1, 5, 10, 20` | default `-2`, buttons `-2, -2, -2, -2` | **no answer** |
| pay buttons after `llSetPayPrice(7, [3, 5, PAY_HIDE, PAY_DEFAULT])` | default `7`, buttons `3, 5, -1, 20`: a `PAY_DEFAULT` button is answered with its default | default `7`, buttons `3, 5, -1, -2`: as the script set them | no answer |
| paying a cube with no script | **taken**: B pays the owner, both are told as for a gift but with type `5008` | nothing | nothing |
| paying the scripted cube | the same, and the script's `money` event fires with B and the amount | nothing; the script hears nothing | nothing |
| paying it with type `5001` (a gift) | taken as type **`5008`**: the grid writes the type itself | nothing | nothing |
| the owner paying its own cube | two replies, A to A, and the `money` event with A | nothing | nothing |

So the pay buttons are the one money answer OpenSim gives, and it hands back
the layout as the script set it, where Second Life has already put the
reference's default amounts (`PAY_BUTTON_DEFAULT_0` … `_3`: 1, 5, 10, 20) in
for every `PAY_DEFAULT`. The reference's pay floater shows a quick-pay button
only for an amount above zero, so on OpenSim an object that sets no buttons
offers none. Neither grid checks the amount paid against the buttons.

## Buying an object

B buys A's cube with `ObjectBuy` into its *Objects* folder, carrying the sale
type and price it read; A sets the sale with `ObjectSaleInfo` and the next
owner's rights with `ObjectPermissions`.

| B buys | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a cube not for sale | nothing | agent alert "/Object Object is not for sale" | nothing |
| a copy for L$ 0, the next owner may not copy | **sold**: the item, its next-owner mask without copy | sold, the same | nothing |
| a copy for L$ 0 | sold: a balance reply to each side (type `5000`, `TRANS_OBJECT_SALE`, L$ 0, `ItemDescription` the object's name; "You paid *A* L$0 for Object." to B, an empty description to A) and the item to B (`UpdateCreateInventoryItem`) in the folder it named | sold: the item, no balance reply | nothing |
| a copy for L$ 1 | sold, the same with L$ 1; A's description "*B* paid you L$1 for Object." | **refused**: a message box (`IM_MESSAGEBOX`, from nobody, no name) "Buying anything for a price other than zero is not implemented" | nothing |
| at a price the sale does not name | refused: alert "Sale of Object cancelled.  Price does not match expected." | L$ 0: agent alert "/Object Object price does not match selected price"; anything else: the message box | nothing |
| as a kind of sale the object is not set to | refused: alert "Sale of Object cancelled.  Sell type (copy,original,contents) does not match expected." | L$ 0: agent alert "/Object Object sell type does not match selected type"; anything else: the message box | nothing |
| the contents (a script) for L$ 0 | sold: the balance replies, and a `BulkUpdateInventory` with a new folder named after the object in B's *Objects* folder | sold: no balance reply; the folder and its item arrive as unasked `InventoryDescendents` — of B's root folder, then of the new folder, with the script in it | nothing |
| one item of the contents (`BuyObjectInventory`) | refused: alert keyed `InventoryNotForSale` | nothing: OpenSim has no handler | nothing |
| the original for L$ 0 | sold: alert keyed `NowOwnObject` ("You are now the owner of object Object"), the balance replies; the cube stays where it is, B's, not for sale | sold, nothing said; the same | nothing |
| the original for L$ 1 | sold, the same with L$ 1 | refused: the message box; the cube stays A's and for sale | nothing |

OpenSim's stock money module refuses every price but L$ 0 before it looks at
the object, so a purchase at the wrong price or of the wrong kind is told the
message box when it names a price, and the specific refusal only at L$ 0.
Neither grid refuses to sell a copy a next owner may not copy: the buyer
gets a copy it cannot copy again.

A rezzed cube reads "last owned by" nobody on Second Life and by its owner on
OpenSim (`ObjectPropertiesFamily`).

## The viewer

- **The balance.** The status bar shows the grid's symbol: the login
  response's `currency`, else `OS$` on an OpenSim grid, else `L$`, and a
  region's `OpenSimExtras` `currency` above both while the agent is there —
  the reference's (Firestorm's OpenSim build) order. Neither grid names a
  symbol, so the local OpenSim reads `OS$0`. This viewer tells an OpenSim
  grid by its `OpenSimExtras`, so the symbol is `L$` until the first region's
  features arrive. (`sl-viewer-kit` `grid_currency`.)
- **Payment notices.** A balance reply with a `TransactionInfo` block raises
  `PaymentSent`, `PaymentFailure` or `PaymentReceived` once the other side's
  name is known, worded from the block as the reference words it ("You paid
  … L$1 for Object."), once per transaction id; one without a block raises
  its description as a `SystemMessage`. Second Life's are the only ones
  measured. (`sl-viewer-notices` `payment_notices`.)
- **OpenSim's refusals.** The message box is shown as the reference shows
  every `IM_MESSAGEBOX`: the `SystemMessageTip` with its text
  (`sl-viewer-notices` `system_messages`). The agent alerts lose their `/`
  as every agent alert does (see [Land § Transactions](land.md#transactions)).
- Second Life's keyed alerts (`NowOwnObject`, `InventoryNotForSale`) raise
  their catalogue notices through the alert host.

## Marketplace

Whether a region grants the Second Life Marketplace's `DirectDelivery`
capability, and what the listing service behind it answers each route for an
avatar with no merchant store. The routes are described in
[Marketplace JSON (SLM)](../comms/marketplace.md).

Measured on 2026-10-19 on aditi (four runs) and the local OpenSim (three runs)
by the census `marketplace-direct-delivery`, one avatar, asking each route raw
(status, media type, redirect, body) and then through our own client's
commands. None of the test accounts has a store, so the routes that change a
listing in place (`PUT /listing/<id>`, `PUT /associate_inventory/<id>`,
`DELETE /listing/<id>`) are not asked: they would name a listing of the
avatar's own, which it has none of. What a merchant is answered is
**unmeasured** and needs a merchant store.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `DirectDelivery` in the seed grant | granted (to a storeless avatar too) | **not granted** | Second Life: granted; OpenSim: withheld |
| `GET /merchant`, no JSON headers | `404`, `text/plain; charset=utf-8`, body `Not found`, no redirect | — | Second Life: the same; OpenSim: — |
| `GET /listings` | the same `404 Not found` | — | the same as its grid |
| `GET /listing/1` (nobody's) | the same `404 Not found` | — | the same as its grid |
| `POST /listings` of a nil listing folder | the same `404 Not found` | — | the same as its grid |
| `GET /census-unknown-route` | the same `404 Not found` | — | the same as its grid |

The merchant probe's `404` is the reference's "not a merchant", but a route
the service does not have answers it too, word for word: for a storeless
avatar, nothing behind the capability tells the service's refusal from a
missing route, and a client cannot learn more than "not a merchant". The fake
grid's Second Life flavour serves exactly that (`SimCaps`'s `DirectDelivery`
handler) and keeps no stores.

## The viewer and the marketplace

- **The client's reading.** Our client reads Second Life's probe as *not a
  merchant* and its listings answer as an SLM error (`404`, the body as the
  message), the reference's `MARKET_FETCH_FAILED`. Without the capability
  (OpenSim) both commands answer a connection failure naming the missing
  capability, as the reference does for an empty capability URL. The census
  holds both readings on all four grids.
- **The viewer** shows no marketplace interface yet: the Content menu has no
  Marketplace Listings entry and the inventory's marketplace menu block is
  left out, so there is nothing to hide on a grid without the capability. The
  listings window, the inventory's marketplace block and the merchant notices
  are the roadmap task `viewer-inventory-marketplace-operations`, which can
  now be built against the measured storeless answer the fake grid's Second
  Life flavour serves.
