export class MiddlemanValidationError extends Error {}
export class MiddlemanConflictError extends Error {}

export const FUSION_CATALOG_ID = "TOKEN_CELL_EXCHANGE";

export type FusionData = {
    SlotID: number,
    EndTime: string,
    ResultCell: string,
    ExchangeID: string
};

export function ParseFusionData(Item: any): FusionData {
    let Data: any;

    try{
        Data = typeof Item?.itemData === "string" ? JSON.parse(Item.itemData) : Item?.itemData;
    }
    catch{
        throw new MiddlemanValidationError("Invalid Middleman fusion token");
    }

    // The exchange UI has three slots and the native exchange code addresses them as 1, 2 and 3.
    // Slot 0 is not a fourth fusion slot; accepting it creates state the client cannot address again.
    if(!Number.isInteger(Data?.SlotID) || Data.SlotID < 1 || Data.SlotID > 3 ||
        typeof Data?.EndTime !== "string" || Number.isNaN(Date.parse(Data.EndTime)) ||
        typeof Data?.ResultCell !== "string" || !Data.ResultCell.startsWith("CELL_") ||
        typeof Data?.ExchangeID !== "string" || Data.ExchangeID.length === 0){
        throw new MiddlemanValidationError("Invalid Middleman fusion token");
    }

    return Data as FusionData;
}

// Matching and removal must never refuse a claim because the token's itemData is not the shape we expect.
function TryParseFusionData(Item: any): FusionData | undefined {
    try{
        return ParseFusionData(Item);
    }
    catch{
        return undefined;
    }
}

export function FusionInstanceId(SlotID: number){
    return `${FUSION_CATALOG_ID}:${SlotID}`;
}

// Some clients use the same generic instance id for every pending exchange. Persist by slot instead so
// several fusions survive at once. Non-fusion items and a fusion token without usable itemData are untouched.
// Strict (throws on bad itemData) for adds and saves; Lenient for removals, which keep the client's id.
export function NormaliseFusionItem(Item: any, Lenient = false){
    if(Item?.catalogId !== FUSION_CATALOG_ID || Item?.itemData == null){
        return Item;
    }

    const Data = Lenient ? TryParseFusionData(Item) : ParseFusionData(Item);

    if(Data != undefined){
        Item.instanceId = FusionInstanceId(Data.SlotID);
    }

    return Item;
}

export function FindFusionItemIndex(InstancedItems: any[], IncomingItem: any){
    if(IncomingItem?.catalogId !== FUSION_CATALOG_ID){
        return -1;
    }

    const Incoming = TryParseFusionData(IncomingItem);

    if(Incoming != undefined){
        const BySlot = InstancedItems.findIndex((Item) => {
            if(Item?.catalogId !== FUSION_CATALOG_ID) return false;
            const Current = TryParseFusionData(Item);
            return Current != undefined && (Current.SlotID === Incoming.SlotID || Current.ExchangeID === Incoming.ExchangeID);
        });

        if(BySlot >= 0){
            return BySlot;
        }
    }

    // Compatibility with a fusion cached before slot-stable ids existed: only safe when there is one.
    if(IncomingItem?.instanceId === FUSION_CATALOG_ID){
        const FusionIndexes = InstancedItems.map((Item, Index) => Item?.catalogId === FUSION_CATALOG_ID ? Index : -1)
            .filter((Index) => Index >= 0);

        if(FusionIndexes.length === 1){
            return FusionIndexes[0];
        }
    }

    return -1;
}

function SameFusion(CurrentItem: any, IncomingItem: any){
    if(CurrentItem?.catalogId !== FUSION_CATALOG_ID || IncomingItem?.catalogId !== FUSION_CATALOG_ID ||
        CurrentItem?.itemData == null || IncomingItem?.itemData == null){
        return false;
    }

    const Current = TryParseFusionData(CurrentItem);
    const Incoming = TryParseFusionData(IncomingItem);

    return Current != undefined && Incoming != undefined &&
        Current.SlotID === Incoming.SlotID &&
        Current.ExchangeID === Incoming.ExchangeID &&
        Current.ResultCell === Incoming.ResultCell;
}

// Speed-up changes EndTime without bumping the inventory item's version. Accept only a move earlier in time;
// the slot, exchange identity and predetermined result are immutable.
export function IsValidFusionSave(CurrentItem: any, IncomingItem: any, Operation: string){
    if(Operation !== "save" && Operation !== "update"){
        return false;
    }

    if(CurrentItem?.updateVersion !== IncomingItem?.updateVersion || !SameFusion(CurrentItem, IncomingItem)){
        return false;
    }

    return Date.parse(ParseFusionData(IncomingItem).EndTime) <= Date.parse(ParseFusionData(CurrentItem).EndTime);
}

// Claiming a completed exchange removes the token the game already holds. The client's copy can lag the
// stored one (an older updateVersion after a speed-up, or itemData we cannot parse), so the generic
// stale-version rule must not reject it: refusing it left finished fusions unclaimable. Removing a
// pending token only loses that fusion; the reveal-time checks live in ValidateFusionCompletions.
export function IsValidFusionRemoval(CurrentItem: any, IncomingItem: any, Operation: string){
    return Operation === "remove" &&
        CurrentItem?.catalogId === FUSION_CATALOG_ID &&
        IncomingItem?.catalogId === FUSION_CATALOG_ID;
}

// Optional strict guard used once the live 1.4.4 transaction shape is confirmed. The server must reveal
// the predetermined result only after EndTime. It intentionally runs inside the inventory transaction.
export function ValidateFusionCompletions(CurrentItems: any[], Removing: any[], AddingStacks: any[], Now = Date.now()){
    for(const Incoming of Removing){
        if(Incoming?.catalogId !== FUSION_CATALOG_ID){
            continue;
        }

        const Index = FindFusionItemIndex(CurrentItems, Incoming);

        if(Index < 0){
            throw new MiddlemanConflictError("Middleman fusion token is no longer pending");
        }

        const Current = CurrentItems[Index];
        const Data = ParseFusionData(Current);

        if(Date.parse(Data.EndTime) > Now){
            throw new MiddlemanConflictError(`Middleman fusion slot ${Data.SlotID} is not complete yet`);
        }

        const Reward = AddingStacks.find((Item) => Item?.catalogId === Data.ResultCell);
        const Quantity = Number(Reward?.quantity ?? 0);

        if(!Number.isSafeInteger(Quantity) || Quantity < 1){
            throw new MiddlemanConflictError(`Middleman fusion slot ${Data.SlotID} did not grant its result cell ${Data.ResultCell}`);
        }

        const OtherCell = AddingStacks.find((Item) => typeof Item?.catalogId === "string" && Item.catalogId.startsWith("CELL_") && Item.catalogId !== Data.ResultCell);

        if(OtherCell != undefined){
            throw new MiddlemanConflictError(`Middleman fusion slot ${Data.SlotID} tried to grant ${OtherCell.catalogId} instead of ${Data.ResultCell}`);
        }
    }
}
