---
title: The in-game store
parent: Findings
nav_order: 11
description: "How the Dauntless 1.4.4 client's store buys an item, read from the executable, and the free store we built from Harmonic's fork: the four routes and the purchase token, the catalogue and its tabs, stacked and instanced grants, which character gets the item, and the switches."
lang: en
ref: findings/store
---

{% assign api_page = site.pages | where: "path", "reference/api.md" | first %}
{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign files_page = site.pages | where: "path", "reference/files.md" | first %}
{% assign game_page = site.pages | where: "path", "reference/game-settings.md" | first %}
{% assign contract_page = site.pages | where: "path", "findings/backend-contract.md" | first %}
{% assign harmonic_page = site.pages | where: "path", "findings/harmonic-fork.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "roadmap.md" | first %}
{% assign admin_page = site.pages | where: "path", "setup/admin.md" | first %}

# The in-game store in 1.4.4
{: .no_toc }

This page describes how the **1.4.4** client's store screen lists offers and buys one, and the store
the metagame now answers it with. The original store sold cosmetics for Platinum, bought with real
money; that store is gone for good. Ours is **free**: every offer costs nothing, and a purchase only
unlocks the cosmetic.

**Status (23 September 2026): built and tested without the game, off by default (`STORE=off`).**
Two things are open before it goes on: the owner's decision whether the store stays free or gets
prices in in-game currency (roadmap 3.7), and a store test in game on our server. With `STORE=off`
the store screen gets the same error as before.

The catalogue, the purchase-token flow, the tab layout and the list of how each item is granted come
from **Harmonic's** Dauntless 1.4.4 fork
([github.com/Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted), commit `895f7c7`),
where Harmonic tried the store in game. We rebuilt the routes on our own inventory, entitlement and audit
code. What we took and what we changed is on [The Harmonic port]({{ harmonic_page.url | relative_url }}).

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Evidence and confidence {#evidence}

| Label | Source |
|:------|:-------|
| **B** | The 1.4.4 executable (`Dauntless-Win64-Shipping.exe`): strings and disassembly. Addresses are virtual addresses with image base `0x140000000`. |
| **K** | The endpoint table in `native/internal-server/dllmain.cpp` and the client's own data tables. |
| **O** | Observed in game by Harmonic on that fork (one client), not yet on our server. |
| **S** | Strong inference: one link in the chain was not traced. |
| **G** | A design decision of ours, where nothing in the client decides it. |
| **C** | Our own code and tests. |

## How the client buys an item {#flow}

The store uses four endpoint keys, all of them already pointed at the metagame by the server DLL (K
`dllmain.cpp`, the `Store*` keys). No DLL change was needed.

| Step | Endpoint key | Request | Reply the client reads |
|:-----|:-------------|:--------|:-----------------------|
| 1. Open a store page | `StoreGetItemByTagEndpoint` | `GET /product/skus/public?requiredTags=<tag>` | A **bare array** of offers (no envelope). The store screen asks for `webstore`; the tags seen on 1.4.4 are listed under [The catalogue](#catalogue). |
| 2. Open the purchase dialog | `StoreGetItemByIdEndpoint` | `GET /product/sku/<sku id>` | One offer. |
| 3. Press Buy | `StorePurchaseItemEndpoint` | `GET /token/<currency>/<sku id>` | `{"purchaseToken": "<token>"}` |
| 4. Confirm | `StorePurchaseItemConfirmEndpoint` | `POST /notification/<currency>?token=<token>` | Any 2xx; we answer 204 with no body. |

- The two purchase calls are made in that order by the purchase code (B `0x140b39657` for the token,
  `0x140b55011` for the confirm). That the `POST` is the confirmation, the point where the purchase
  happens, is strong inference (S): nothing else follows it.
- The currency in the path is `platinum` for every offer we sell (S: the client picks it from the
  offer's price fields; any other currency is refused with 400).
- The offer's price fields are **flat**: `platinumPrice`, `platinumSalePrice`, `cellDustPrice`,
  `prestigePrice`, `event01Price`, `steelMarksPrice` and `gildedMarksPrice` (B, the field names in the
  1.4.4 executable, read by Harmonic). The newer `prices: [{currencyId, price}]` array of later clients
  has no reader in 1.4.4, so an offer sent that way has no price the client recognises and is dropped
  (S, from Harmonic's notes: the store stays empty).
- `remaining` tells the client whether the offer can still be bought. We send 0 once the character
  holds every item of the offer and the account owns every entitlement it grants.

Whether the new item shows at once or only after the next inventory read (a relog or a map load) is
for the in-game test to show.

## The catalogue {#catalogue}

The offers live in `legacy/UndauntedMetagame/src/vendor/store_catalog.json`, keyed by the tag the client asks
for. Nothing the client sends decides what an offer grants or costs: the metagame looks the offer up
by its id every time.

| Tag | Offers | What |
|:----|:-------|:-----|
| `webstore` | 200 | The store screen: armour, weapon skins, lanterns, emotes and arrivals, flares and sigils, banner fabrics and standards, dyes, sheens and hair tints, and the bounty-token bundle (hidden, see below). |
| `season09b_pass` | 1 | `season09b_premium`, the Elite Hunt Pass. Every account already owns it (`ENTITLEMENTS_DEFAULT`), so it shows as owned. |
| `season09b_rank` | 0 | Hunt Pass rank skips. Served empty: a rank skip would be a progression grant, not an item (see below). |
| `loadout_slots`, `fountain_daily_free_bundle` | 0 | Tags the 1.4.4 client asks for; nothing is sold under them. |

These five are the tags Harmonic saw the 1.4.4 client ask for (O). Any other tag gets an empty list and
a warning line. Every offer has `platinumPrice` 0. A few facts about the list, from Harmonic's work:

- **Store tabs.** Each offer carries category tags besides `webstore` (`skin_armour`, `feature`,
  `social_emote`, `personality_fabric` and so on). The store's tab bar indexes the full list of tabs,
  hidden ones included, so an empty tab placed before a filled one shifts every later tab's contents
  (O: EMOTES opened weapons). The catalogue therefore gives the tabs `your_offers`, `supplies_boost`,
  `personality_stylekit` and `personality_character` at least one offer each.
- **Tile art.** The client draws a tile from its own store item table (`StoreItemsTable`, 1,405 rows keyed
  by SKU id). An offer whose id is not a row with an image shows a fallback picture, and the tiles are
  a fixed 2:1 frame, so square Hunt Pass icons show stretched. The storefront therefore sells only SKU
  ids whose tile image is 2:1; a test holds every offer to the list of such ids
  (`legacy/UndauntedMetagame/test/data/store_art_skus.json`, 994 ids, no images).
- **Sheens and hair tints are entitlements**, not items. Their SKU-to-entitlement pairs come from the
  client's own tables (the dye sheen table and the premium hair colour table): five sheens
  (`ent_dye_sheen_glossy` and four more) and four hair-tint offers.
- **Dye palette bundles are not sold.** The client defines their sets but assigns no dyes to them, so
  their contents lived only on the old servers.

The list holds identifiers and short English names read from the client, and no game assets; see
[Credits and license]({{ '/legal.html' | relative_url }}#no-game-files).

## Stacked and instanced grants {#grants}

An item enters the inventory in one of two ways: as a **stack** (a catalogue id with a quantity) or as
an **instance** (an item with an instance id of its own). The client's item catalogue decides which,
through each item's stackable flag, and a wrong kind leaves the item invisible or breaks the
inventory. `legacy/UndauntedMetagame/src/vendor/store_item_kinds.json` records the flag for each of the 211
items the store sells (95 stacked, 116 instanced), read from the client's catalogue by Harmonic. A rule
by name prefix gets individual items wrong: most weapon skins and a few arrivals are instanced, while a
few fabrics and lanterns are stacked.

- **Stacked armour was seen working in game** (O). The store sells armour only where it is stackable.
- **Instanced grants are not proven in game**, not even by Harmonic. The instance id is
  `sha256(<token hash>:<catalogue id>)`, cut to 32 hex characters, so a retried confirm names the same
  instance. The in-game test buys one instanced weapon skin and checks that it shows and can be
  equipped after a relog.
- Only these kinds of item are ever handed out, whatever an offer lists: `AR_` `WP_` `EM_` `DYE_`
  `QI_FLARE_` `BNC_FABRIC_` `BNC_STANDARD_` `BNC_SIGIL_` `LT_`, one of each, plus the bounty-token
  bundle. An offer that names anything else (a currency, a boost, a real weapon) is refused (G).

## Which character gets the purchase {#character}

The purchase routes name no character, and an account on our server may have several (each
`POST /character` makes one). The metagame binds the purchase token to the account's **active
character** when the token is issued: the character whose data was saved most recently (the game
saves the character it plays about once a minute, and each save is a row in `characterhistory`);
without any saved version, the later `lastModifiedDate` and then the higher `updateVersion` decide.
That this is the character on screen is a strong inference (S). An account with no character gets 409
for the token, and a confirm is refused (403) when the token's character no longer belongs to the
account.

Harmonic's version refused accounts with more than one character. Before `STORE=free` goes on for
real players, the owner counts the characters per account on the live database (the read-only query
is on [Run it for a group]({{ admin_page.url | relative_url }}#switching-features-on)); how many
accounts have more than one has not been counted yet.

## The purchase, step by step {#redeem}

1. **The token.** `GET /token/platinum/<sku>` checks that the offer exists, is free and grants only
   allowed things, that the player does not already own everything it grants (409; the bounty-token
   bundle is never owned) and that the account has had fewer than 60 tokens in the last 10 minutes
   (409), then stores a row in `storepurchases`: the SHA-256 of a random 64-hex token (the token itself
   is never stored), the account, the active character, the SKU, a hash of the offer as it is now, and
   an expiry 10 minutes later. The account's own unredeemed tokens past their expiry are deleted when it
   gets a new one; the whole table is swept at every start and then at most once an hour (expired
   tokens never redeemed, and receipts redeemed more than 30 days ago).
2. **The confirm.** `POST /notification/platinum?token=<token>` runs in one database transaction:
   - another account's token or an unknown one: 403; a token whose character moved: 403; an expired
     token: 410; an offer that changed since the token, or is no longer sold: 409;
   - a token that was **already redeemed**: 204 again, and nothing is granted (a retry after a lost
     answer);
   - the items go through the same inventory code as `POST /inventory`, with the caller `store`, the
     source `store:<sku>` and the transaction id `store:<token hash>`: one row in the transaction ledger
     and one `inventorylog` row per item. Something the character already holds is not granted again;
   - the entitlements go through the same code as the game server's grants, with the source
     `store:<sku>`;
   - the token row is marked redeemed. If anything fails, nothing of it is kept. The row is the receipt
     a retried confirm is answered from; it is deleted 30 days after the redeem, while the grant stays
     in `inventorylog` (kept forever) and `entitlements`.
3. **Ownership.** An offer reads `remaining: 0` once every item is held and every entitlement is
   active. A revoked or expired entitlement can therefore be bought again, and the Elite pass reads as
   owned through the default entitlements.

Every route acts for the player whose token it carries: without a token 401, and a game server's key
alone gets 403. Purchases can be traced (`inventorylog.caller = 'store'`, `entitlements.source LIKE
'store:%'`) and taken back with the existing admin routes ([HTTP API]({{ api_page.url | relative_url }}#undaunted-api)).

## What we left out, and why {#left-out}

| Left out | Why |
|:---------|:----|
| Harmonic's `season09b_10_ranks` offer (ten Hunt Pass ranks) | It granted `CURRENCY_PRESTIGE`, which the fork's own store code could not sell (no grant kind: 409). A real rank skip is a progression grant through the rank rules, not an item. |
| Prices | Every offer is free until the owner decides otherwise (roadmap 3.7). The code refuses any offer whose `platinumPrice` is not 0. |
| The bounty-token bundle, by default | `bundle_currency_bounty_small` gives 20 `TOKEN_BOUNTY_DRAFT_PREMIUM`, the premium bounty token that lasts into the next season, and may be bought any number of times: unlimited free premium bounty drafts. It is listed only with `STORE_REPEATABLE_TOKENS=1`, an owner decision. A token issued while it was on is refused (409) after it is switched off. |
| The Trials, event and prestige stores, the cell-dust exchange, loadout-slot purchases | The same service ran them; nothing is built for them yet (roadmap 3.7, 3.9). |

## Switches {#switches}

| Setting | Default | What it does |
|:--------|:--------|:-------------|
| `STORE` | `off` | `off`: the store screen gets the old 400 (`{"code": "400", "message": "The store is not available on Dauntless Revived yet."}`) and the three purchase routes the 404 they always got. `free`: the catalogue above, the token and the confirm. |
| `STORE_REPEATABLE_TOKENS` | off | `1` lists and sells the bounty-token bundle, unlimited. |

The full descriptions are on [Configuration]({{ config_page.url | relative_url }}#metagame-store), the
routes on [HTTP API]({{ api_page.url | relative_url }}#store), the table on
[Files and data]({{ files_page.url | relative_url }}), and the catalogue files on
[Game settings]({{ game_page.url | relative_url }}#store-catalogue).

## The in-game test, and what is still open {#open}

Before `STORE=free` becomes the default, with a throwaway account on the rented server:

1. The owner decides free or priced (3.7), and whether the bounty-token bundle is sold.
2. Count the characters per account on the live database (the purchase goes to the character saved
   last).
3. Set `STORE=free` in the metagame's settings and restart it when nobody is playing.
4. Open every tab: each shows its own kind of item (EMOTES shows emotes).
5. Buy one stacked armour piece, one instanced weapon skin, one lantern and one sheen (and the token
   bundle, if allowed). The log shows `Store purchase token for <sku> issued to <account>` and
   `Store purchase <sku> for <account> (character <id>): N item(s), M entitlement(s)`.
6. Relog: everything bought is visible and can be equipped, and the offers read as owned.
7. Finish a hunt: no inventory 409 afterwards.

| Open point | Label |
|:-----------|:------|
| The `POST /notification` is the confirmation | S |
| Instanced items bought in the store show and can be equipped | not seen yet |
| The active character is the one on screen | S |
| The item shows at once, without a relog | not seen yet |
| Free or priced | the owner's decision (3.7) |

The history of the store item is on the [roadmap]({{ roadmap_page.url | relative_url }}) (3.7), and
the [backend contract]({{ contract_page.url | relative_url }}) describes the other services the
client talks to.
