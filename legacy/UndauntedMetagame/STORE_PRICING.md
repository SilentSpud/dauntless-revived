# Store pricing

How to price store offers in earned in-game currency, and how the charge actually works.

## Supported currencies

Set exactly one of these price fields on an offer in `src/vendor/store_catalog.json` to a
non-negative integer, and set every other price field (including `platinumPrice`) to `null`.
Whichever single field is set determines what currency the offer is sold in.

| Price field | Currency key (client/URL) | Deducted from (inventory stack) |
|---|---|---|
| `platinumPrice` | `platinum` | `CURRENCY_PLATINUM_UNIV` (earned via the Hunt Pass) |
| `cellDustPrice` | `celldust` | `CURRENCY_CELLDUST` |
| `steelMarksPrice` | `markssteel` | `CURRENCY_MARKS_STEEL` |
| `gildedMarksPrice` | `marksgilded` | `CURRENCY_MARKS_GILDED` |
| `prestigePrice` | `prestige` | `CURRENCY_PRESTIGE` |

`event01Price` is not wired up: it maps to a seasonal event currency and which one is ambiguous.
Leave it `null`; setting it alone is refused.

The currency keys above (`platinum`, `celldust`, `markssteel`, `marksgilded`, `prestige`) were
confirmed by disassembling the 1.4.4 client's store purchase code, not guessed.

## Example

Free (current default, every offer ships this way):

```json
"platinumPrice": 0,
"cellDustPrice": null,
"steelMarksPrice": null,
"gildedMarksPrice": null,
"prestigePrice": null
```

Priced at 500 Cell Dust instead:

```json
"platinumPrice": null,
"cellDustPrice": 500,
"steelMarksPrice": null,
"gildedMarksPrice": null,
"prestigePrice": null
```

## How the charge works (`src/controllers/freestore.ts`)

1. `ResolveOfferPricing` reads the offer and requires **exactly one** price field to be set; more
   than one, or none, is treated as a catalogue bug and refused (500) rather than guessed at.
2. `GET /token/<currency>/<sku>` (token issuance) refuses the request (400) if `<currency>` doesn't
   match the offer's own priced currency, and refuses (409) if the character's current balance in
   that currency is lower than the price.
3. `POST /notification/<currency>?token=<token>` (redeem) re-checks the balance inside the same
   database transaction as the grant, then removes the price and adds the purchased item(s) in one
   atomic inventory transaction. A short balance at this point (e.g. spent elsewhere between token
   issuance and redeem) refuses the purchase (409) and grants nothing.
4. A retried redeem (same token twice) never charges or grants twice — the purchase row is marked
   redeemed and answered from after the first success.

## Turning the store on

Pricing works regardless, but the store only answers requests with `STORE=free` set in the
metagame's environment. See `docs/setup/admin.md` for how to switch it on.
