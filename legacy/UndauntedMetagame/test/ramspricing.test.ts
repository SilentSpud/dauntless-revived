import {test} from 'node:test';
import assert from 'node:assert/strict';
import {PriceInRams, RamsWireOffer, RamsWireCurrency} from '../src/controllers/ramspricing';
import {WithEnv} from './appenv';
import type {StoreOffer} from '../src/controllers/freestore';

test('pilot price tiers, bundle cap and account isolation',()=>WithEnv({STORE_RAMS_TEST_ACCOUNT:'pilot'},()=>{
    const offer=(ids:string[]):StoreOffer=>({id:'test',tags:['webstore'],platinumPrice:0,remaining:1,entitlements:[],items:ids.map(catalogId=>({catalogId,quantity:1}))});
    for(const [ids,price] of [
        [['QI_FLARE_TEST'],250], [['BNC_SIGIL_TEST'],250], [['EM_TEST'],750],
        [['WP_TEST'],2500], [['DYE_TEST'],5000],
        [['AR_HEAD','AR_BODY','AR_ARMS','AR_LEGS'],5000],
        [['WP_TEST','EM_TEST'],3250], [['WP_TEST','DYE_TEST'],5000]
    ] as [string[],number][]){
        const original=offer(ids);
        assert.equal(PriceInRams(original,'pilot').notesPrice,price);
        assert.equal(PriceInRams(original,'other'),original);
        assert.equal(original.platinumPrice,0);
    }
    const unsupported=offer(['CURRENCY_NOTES']);
    assert.equal(PriceInRams(unsupported,'pilot'),unsupported);
    const sheen:StoreOffer={...offer([]),tags:['webstore','dye_armour'],entitlements:[{name:'ent_dye_sheen_glossy'}]};
    assert.equal(PriceInRams(sheen,'pilot').notesPrice,5000);
    assert.equal(PriceInRams(sheen,'other'),sheen);
}));

test('native pilot transport preserves canonical Rams accounting and isolates other accounts',()=>WithEnv({STORE_RAMS_TEST_ACCOUNT:'pilot',STORE_RAMS_EVENT_TRANSPORT:'1'},()=>{
    const offer:StoreOffer={id:'test',tags:['webstore'],notesPrice:5000,remaining:1,entitlements:[],items:[]};
    const wire=RamsWireOffer(offer,'pilot');
    assert.equal(wire.event01Price,5000);
    assert.equal(wire.notesPrice,undefined);
    assert.ok(wire.tags.includes('rams-store-pilot'));
    assert.equal(offer.notesPrice,5000);
    assert.equal(offer.event01Price,undefined);
    assert.equal(RamsWireOffer(offer,'other'),offer);
    assert.equal(RamsWireCurrency('event01','pilot'),'notes');
    assert.equal(RamsWireCurrency('event_01','pilot'),'notes');
    assert.equal(RamsWireCurrency('event_01','other'),'event_01');
    assert.equal(RamsWireCurrency('event01','other'),'event01');
    assert.equal(RamsWireCurrency('platinum','pilot'),'platinum');
    const vendor={...offer,notesPrice:undefined,event01Price:5};
    assert.equal(RamsWireOffer(vendor,'pilot'),vendor);
}));

test('wire adapter remains disabled without explicit native pilot switch',()=>WithEnv({STORE_RAMS_TEST_ACCOUNT:'pilot',STORE_RAMS_EVENT_TRANSPORT:undefined},()=>{
    const offer:StoreOffer={id:'test',tags:['webstore'],notesPrice:250,remaining:1,entitlements:[],items:[]};
    assert.equal(RamsWireOffer(offer,'pilot'),offer);
    assert.equal(RamsWireCurrency('event01','pilot'),'event01');
}));
