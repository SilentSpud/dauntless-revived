import { eq, inArray, max } from "drizzle-orm";
import { characterhistory, characters } from "../db/schema";
import { Tx } from "./savehistory";

// The character an account plays now, for the routes that name no character: the store's purchases and
// the currency balance (/balance, /reconcile). An account may hold several characters (CreateCharacterForUid
// makes one per POST /character). The one whose data was saved last wins: the client saves the playing
// character every minute or so, and each save is a characterhistory row (ids only grow). Without any saved
// version, the later lastModifiedDate (a day, "Sep 23, 2026") and then the higher updateVersion decide.
// That this is the character on screen is a strong inference, not a capture.
export function GetActiveCharacter(tx: Tx, AccountId: string){
    const Owned = tx.select().from(characters).where(eq(characters.userId, AccountId)).all();

    if(Owned.length <= 1){
        return Owned[0];
    }

    const LatestSave = new Map(tx.select({characterId: characterhistory.characterId, Id: max(characterhistory.id)}).from(characterhistory)
        .where(inArray(characterhistory.characterId, Owned.map((Row) => Row.characterId)))
        .groupBy(characterhistory.characterId).all()
        .map((Row) => [Row.characterId, Row.Id ?? 0]));

    const Day = (Text: string) => {
        const Parsed = Date.parse(Text);
        return Number.isNaN(Parsed) ? 0 : Parsed;
    };

    return [...Owned].sort((A, B) =>
        (LatestSave.get(B.characterId) ?? 0) - (LatestSave.get(A.characterId) ?? 0) ||
        Day(B.lastModifiedDate) - Day(A.lastModifiedDate) ||
        B.updateVersion - A.updateVersion ||
        A.characterId.localeCompare(B.characterId))[0];
}
