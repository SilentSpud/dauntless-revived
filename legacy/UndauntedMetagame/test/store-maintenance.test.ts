import { RemoveTestDb } from './setup';
import './authenv';
import { after, afterEach, beforeEach, test } from 'node:test';
import assert from 'node:assert/strict';
import { GetDb } from '../src/db';
import { CreateStorePurchase, GetStoreOffer, ListStoreOffers, RedeemStorePurchase } from '../src/controllers/freestore';
import { RevokeStorePurchasesBefore } from '../src/controllers/storerevocation';
import { ApplyInventoryTransactionInTx } from '../src/controllers/inventory';
import { HasActiveEntitlement, GrantEntitlementInTx } from '../src/controllers/entitlements';
import { MakePlayer, StackQuantity } from './helpers';

after(() => RemoveTestDb(() => GetDb().$client.close()));

beforeEach(() => {
    process.env.STORE = 'free';
    process.env.TRIALS_STORE = '0';
    process.env.MIDDLEMAN_STORE = '0';
});

afterEach(() => {
    delete process.env.STORE;
    delete process.env.TRIALS_STORE;
    delete process.env.MIDDLEMAN_STORE;
});

test('curated shop has exactly 30 free cosmetics and blocks excluded direct purchases', async () => {
    const player = await MakePlayer();
    process.env.STORE_CATALOG_PROFILE = 'curated30';
    process.env.STORE_REPEATABLE_TOKENS = '1';
    try {
        const offers = ListStoreOffers(player.UserId, 'webstore');
        assert.equal(offers.length, 30);
        assert.equal(new Set(offers.map(o => o.id)).size, 30);
        assert.ok(offers.some(o => o.id === "bundle_dyes_palette_01"));
        assert.ok(offers.every(o => !o.id.startsWith("single_dye_sheen_") && !(o.entitlements ?? []).some(e => e.name.startsWith("ent_dye_sheen_"))));
        for (const offer of offers) {
            assert.equal(offer.platinumPrice, 0);
            assert.ok(!(offer.items ?? []).some(i => i.catalogId.startsWith('TOKEN_')));
        }
        assert.throws(() => GetStoreOffer(player.UserId, 'bundle_currency_bounty_small'), /Unknown store offer/);
        assert.throws(() => CreateStorePurchase(player.UserId, 'platinum', 'single_weapon_kats_claw'), /Unknown store offer/);
    } finally { delete process.env.STORE_CATALOG_PROFILE; delete process.env.STORE_REPEATABLE_TOKENS; }
});

test('revocation removes only recorded grants, preserves earned ownership, and cannot run twice', async () => {
    const p = await MakePlayer();
    const sku = 'bundle_armour_iron';
    const offer = GetStoreOffer(p.UserId, sku);
    const preowned = offer.items![0].catalogId;
    const purchased = offer.items![1].catalogId;
    const grant = (id: string, transaction: string) => GetDb().transaction(tx => ApplyInventoryTransactionInTx(tx, {
        UserId:p.UserId, CharacterId:p.CharacterId, TransactionId:transaction,
        StackedItemsToAdd:[{catalogId:id,quantity:1}]
    }, {Caller:'gameserver',Source:'hunt-reward'}));
    grant(preowned, 'earned-before');
    const buy = (id: string) => RedeemStorePurchase(p.UserId,'platinum',CreateStorePurchase(p.UserId,'platinum',id).purchaseToken);
    buy(sku);
    grant(purchased, 'earned-after');
    const weapon = 'single_weapon_unseen_sword';
    buy(weapon);
    const hairTint = GetStoreOffer(p.UserId,'single_dye_hairtint_01').entitlements![0].name;
    buy('single_dye_hairtint_01');
    GetDb().transaction(tx => GrantEntitlementInTx(tx,p.UserId,'earned-entitlement',0,'hunt'));
    const cutoff = new Date().toISOString();
    const preview = RevokeStorePurchasesBefore(cutoff);
    assert.equal(preview.purchases,3);
    assert.equal(preview.instances,1);
    assert.equal(preview.entitlements,1);
    assert.equal(StackQuantity(p.CharacterId,purchased),2);
    assert.deepEqual(RevokeStorePurchasesBefore(cutoff,true),preview);
    assert.equal(StackQuantity(p.CharacterId,preowned),1);
    assert.equal(StackQuantity(p.CharacterId,purchased),1);
    assert.equal(GetDb().transaction(tx=>HasActiveEntitlement(tx,p.UserId,hairTint)),false);
    assert.equal(GetDb().transaction(tx=>HasActiveEntitlement(tx,p.UserId,'earned-entitlement')),true);
    assert.equal(RevokeStorePurchasesBefore(cutoff,true).purchases,0);
    assert.equal(StackQuantity(p.CharacterId,purchased),1);
    // A new free-shop purchase stays valid after the maintenance cutoff.
    await new Promise(resolve=>setTimeout(resolve,5));
    buy(weapon);
    assert.equal(RevokeStorePurchasesBefore(cutoff,true).instances,0);
    assert.equal(GetStoreOffer(p.UserId,weapon).remaining,0);
});
