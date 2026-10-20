import {createHash} from 'node:crypto';
import type {StoreOffer} from './freestore';
import {RamsEnabled} from './ramspricing';
import {WeeklyStoreWindow} from './storeclock';

// A seed refreshes current stock without changing the weekly schedule.
export function ShopLayout(offers: StoreOffer[], accountId: string, now = new Date()): StoreOffer[] {
    if(!RamsEnabled(accountId) || process.env.STORE_WEEKLY_ROTATION !== '1') return offers;
    const window=WeeklyStoreWindow(now);
    const week=window.index;
    const seed=process.env.STORE_ROTATION_SEED ?? '';
    const ranked=offers.map(o=>({id:o.id,rank:createHash('sha256').update(`${seed}:${o.id}`).digest('hex')}))
        .sort((a,b)=>a.rank.localeCompare(b.rank)||a.id.localeCompare(b.id));
    const cleaned=offers.map(offer=>{
        const tags=offer.tags.filter(tag=>tag!=='feature'
            && !(tag==='personality_character' && offer.tags.includes('social_emote'))
            && !(tag==='personality_stylekit' && offer.tags.includes('dye_armour'))
            && !(tag==='personality_standard' && offer.items?.every(item=>item.catalogId.startsWith('BNC_FABRIC_'))));
        return {...offer,tags};
    });
    const categories=new Map<string,StoreOffer[]>();
    const byId=new Map(cleaned.map(o=>[o.id,o]));
    const selected:StoreOffer[]=[];
    for(const row of ranked){
        const offer=byId.get(row.id)!;
        const category=offer.tags.find(t=>/^(skin_|social_|dye_|personality_|supplies_)/.test(t)) ?? 'other';
        const group=categories.get(category) ?? [];
        group.push(offer);
        categories.set(category,group);
    }
    // Consecutive three-item windows avoid repeats when the category has six or more offers.
    for(const group of categories.values()){
        const count=Math.min(3,group.length);
        const start=((week*count)%group.length+group.length)%group.length;
        for(let i=0;i<count;i++) selected.push(group[(start+i)%group.length]);
    }
    const featured=new Set(selected.slice(0,8).map(o=>o.id));
    return selected.map(o=>({...o,
        tags:featured.has(o.id)?[...o.tags,'feature']:o.tags,
        availableFrom:new Date(window.start).toISOString(),
        availableTo:new Date(window.end).toISOString()
    }));
}
