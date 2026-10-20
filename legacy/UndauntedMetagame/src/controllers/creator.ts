import { eq } from "drizzle-orm";
import { GetDb } from "../db";
import { inventory, onetimejobs } from "../db/schema";
import { GetActiveCharacter } from "./activecharacter";
import { StoreError } from "./freestore";
import { ApplyInventoryTransactionInTx } from "./inventory";

const CODE = "Void";
export const CREATOR_REWARD_RAMS = 10000;
const ReceiptId = (AccountId: string) => `creator:void:${AccountId}`;
const Reply = (Supported: boolean) => ({
    success: Supported,
    slug: Supported ? CODE : "",
    expirationDate: "2099-01-01T00:00:00.000Z"
});

// Permanent account receipt: inventory transaction receipts expire and are character-scoped.
export function GetCreatorSupport(AccountId: string){
    return Reply(GetDb().select().from(onetimejobs).where(eq(onetimejobs.jobId, ReceiptId(AccountId))).get() !== undefined);
}

export function RedeemCreatorCode(AccountId: string, Slug: unknown){
    if(typeof Slug !== "string" || Slug.length > 64 || Slug.trim().toLowerCase() !== "void"){
        throw new StoreError(404, "Creator code not found");
    }

    return GetDb().transaction((tx) => {
        if(tx.select().from(onetimejobs).where(eq(onetimejobs.jobId, ReceiptId(AccountId))).get()){
            throw new StoreError(409, "Void has already been redeemed on this account");
        }
        const Character = GetActiveCharacter(tx, AccountId);
        if(!Character) throw new StoreError(409, "Create a character before redeeming Void");

        const Saved = tx.select({stackedItems: inventory.stackedItems}).from(inventory).where(eq(inventory.characterId, Character.characterId)).get();
        const Balance = Saved ? JSON.parse(Saved.stackedItems).find((Item: any) => Item.catalogId === "CURRENCY_NOTES")?.quantity ?? 0 : 0;
        // Native inventory quantities are signed 32-bit integers.
        if(!Number.isSafeInteger(Balance) || Balance < 0 || Balance > 2147483647 - CREATOR_REWARD_RAMS){
            throw new StoreError(409, "Rams balance cannot receive this reward");
        }

        tx.insert(onetimejobs).values({jobId: ReceiptId(AccountId), doneDate: new Date().toISOString()}).run();
        ApplyInventoryTransactionInTx(tx, {
            UserId: AccountId,
            CharacterId: Character.characterId,
            TransactionId: ReceiptId(AccountId),
            StackedItemsToAdd: [{catalogId: "CURRENCY_NOTES", quantity: CREATOR_REWARD_RAMS}]
        }, {Caller: "store", Source: "creator:Void"});

        return {...Reply(true), grantedRams: CREATOR_REWARD_RAMS, refreshInventory: true};
    }, {behavior: "immediate"});
}
