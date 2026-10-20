import { eq } from "drizzle-orm";
import { GetDb } from "../src/db";
import { characters, inventory, users } from "../src/db/schema";
import { CreateCharacterForUid } from "../src/controllers/character";

let Counter = 0;

export async function MakePlayer(){
    Counter++;
    const UserId = `UID-test-${process.pid}-${Counter}`;

    await GetDb().insert(users).values({userId: UserId, name: `Tester${Counter}`, notes: 0});
    const Character = await CreateCharacterForUid(UserId, `Tester${Counter}`);

    return {UserId, CharacterId: Character.id as string};
}

export function ReadCharacter(CharacterId: string){
    return GetDb().select().from(characters).where(eq(characters.characterId, CharacterId)).get()!;
}

export function ReadStacks(CharacterId: string): any[]{
    const Row = GetDb().select().from(inventory).where(eq(inventory.characterId, CharacterId)).get();

    return Row == undefined ? [] : JSON.parse(Row.stackedItems);
}

export function StackQuantity(CharacterId: string, CatalogId: string){
    return ReadStacks(CharacterId).find((Item) => Item.catalogId === CatalogId)?.quantity ?? 0;
}

export function Count(Table: string, Where = "1=1", ...Params: unknown[]): number{
    return (GetDb().$client.prepare(`select count(*) n from ${Table} where ${Where}`).get(...Params) as any).n;
}
