import type { StoreOffer } from './freestore';

export function RamsEnabled(accountId: string): boolean {
    return process.env.STORE_RAMS === '1' || (!!process.env.STORE_RAMS_TEST_ACCOUNT && accountId === process.env.STORE_RAMS_TEST_ACCOUNT);
}

// The old client parses event01Price, but not notesPrice. Only the matching
// native pilot may use this wire adapter; accounting always stays in Rams.
export function RamsWireOffer(offer: StoreOffer, accountId: string): StoreOffer {
    if (!RamsWireEnabled(accountId) || typeof offer.notesPrice !== 'number') return offer;
    const result: StoreOffer = {...offer, event01Price: offer.notesPrice, tags: [...offer.tags, 'rams-store-pilot']};
    delete result.notesPrice;
    return result;
}

function RamsWireEnabled(accountId: string): boolean {
    return process.env.STORE_RAMS_EVENT_TRANSPORT === '1'
        && RamsEnabled(accountId);
}

export function RamsWireCurrency(currency: string, accountId: string): string {
    return RamsWireEnabled(accountId) && ['event01', 'event_01'].includes(currency.toLowerCase()) ? 'notes' : currency;
}

// Pilot only. Never enable globally until the 1.4.4 client currency UI is verified.
export function PriceInRams(offer: StoreOffer, accountId: string): StoreOffer {
    if (!RamsEnabled(accountId) || !offer.tags.includes('webstore')) return offer;
    if(offer.entitlements?.length){
        if(!offer.tags.some(t=>t==='dye_armour'||t==='dye_tint') || offer.items?.length) return offer;
        const result={...offer,notesPrice:5000,displayDescription:'Purchase for 5,000 Rams.'};
        delete result.platinumPrice;
        return result;
    }
    if(!offer.items?.length) return offer;
    const prices = offer.items.map(item => {
        if (item.quantity !== 1) return undefined;
        const id = item.catalogId;
        if (id.startsWith('DYE_')) return 5000;
        if (id.startsWith('WP_')) return 2500;
        if (['BNC_SIGIL_', 'QI_FLARE_'].some(prefix => id.startsWith(prefix))) return 250;
        if (['AR_', 'EM_', 'BNC_FABRIC_', 'BNC_STANDARD_', 'LT_'].some(prefix => id.startsWith(prefix))) return 750;
        return undefined;
    });
    // Unsupported offers remain unchanged rather than inventing prices for currencies or consumables.
    if (prices.some(price => price === undefined)) return offer;
    const fullKit = offer.items.filter(item => item.catalogId.startsWith('AR_')).length >= 4;
    const price = fullKit ? 5000 : Math.min(5000, (prices as number[]).reduce((sum, value) => sum + value, 0));
    const result = {...offer, notesPrice: price, displayDescription: `Purchase for ${price.toLocaleString('en-US')} Rams.`};
    delete result.platinumPrice;
    return result;
}
