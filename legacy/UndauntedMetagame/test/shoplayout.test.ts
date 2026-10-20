import {test} from 'node:test';
import assert from 'node:assert/strict';
import {ShopLayout} from '../src/controllers/shoplayout';
import {WithEnv} from './appenv';
import catalog from '../src/vendor/store_catalog.json';
import type {StoreOffer} from '../src/controllers/freestore';

test('every sufficiently stocked category changes all three offers each week',()=>WithEnv({STORE_RAMS:'1',STORE_WEEKLY_ROTATION:'1',STORE_ROTATION_SEED:'weekly'},()=>{
    const offers=catalog.webstore as StoreOffer[];
    const a=ShopLayout(offers,'player',new Date('2026-10-17T00:00:00Z'));
    const b=ShopLayout(offers,'player',new Date('2026-10-23T12:00:00Z'));
    const category=(o:StoreOffer)=>o.tags.find(t=>/^(skin_|social_|dye_|personality_|supplies_)/.test(t)) ?? 'other';
    for(const c of new Set(a.map(category))){
        if(offers.filter(o=>category(o)===c).length<6)continue;
        const prior=new Set(a.filter(o=>category(o)===c).map(o=>o.id));
        assert.ok(b.filter(o=>category(o)===c).every(o=>!prior.has(o.id)),c);
    }
}));

test('manual seed refresh changes stock and retains automatic weekly rotation',()=>WithEnv({STORE_RAMS:'1',STORE_WEEKLY_ROTATION:'1',STORE_ROTATION_SEED:''},()=>{
    const offers=catalog.webstore as StoreOffer[];
    const now=new Date('2026-10-17T04:00:00Z');
    const previous=ShopLayout(offers,'player',now);
    WithEnv({STORE_ROTATION_SEED:'refresh-20261017'},()=>{
        const fresh=ShopLayout(offers,'player',now);
        assert.notDeepEqual(fresh,previous);
        assert.deepEqual(fresh,ShopLayout(offers,'another-player',now));
        assert.deepEqual(fresh,ShopLayout(offers,'player',new Date('2026-10-18T23:59:59Z')));
        assert.notDeepEqual(fresh,ShopLayout(offers,'player',new Date('2026-10-23T12:00:00Z')));
        assert.ok(fresh.every(o=>offers.some(original=>original.id===o.id)));
    });
}));

test('weekly stock is stable, rotates, preserves prices and corrects category tags',()=>WithEnv({STORE_RAMS_TEST_ACCOUNT:'pilot',STORE_WEEKLY_ROTATION:'1'},()=>{
    const offers=catalog.webstore as StoreOffer[];
    const day=new Date('2026-10-17T00:00:00Z');
    const first=ShopLayout(offers,'pilot',day);
    const featured=(xs:StoreOffer[])=>xs.filter(o=>o.tags.includes('feature')).map(o=>o.id);
    assert.equal(featured(first).length,8);
    assert.deepEqual(first,ShopLayout(offers,'pilot',new Date('2026-10-17T23:59:59Z')));
    assert.notDeepEqual(featured(first),featured(ShopLayout(offers,'pilot',new Date('2026-10-23T12:00:00Z'))));
    assert.ok(first.length < offers.length);
    for(const o of first) assert.equal(o.platinumPrice,offers.find(x=>x.id===o.id)!.platinumPrice);
    assert.equal(ShopLayout(offers,'other',day),offers);
    for(const offer of first){
        if(offer.tags.includes('social_emote')) assert.ok(!offer.tags.includes('personality_character'));
        if(offer.tags.includes('dye_armour')) assert.ok(!offer.tags.includes('personality_stylekit'));
        if(offer.items?.length && offer.items.every(i=>i.catalogId.startsWith('BNC_FABRIC_'))) assert.ok(!offer.tags.includes('personality_standard'));
    }
}));
