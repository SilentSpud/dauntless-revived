import { and, eq, sql } from "drizzle-orm";
import { GetDb } from "../db";
import { loadouts, loadoutslots } from "../db/schema";
import { logger } from "../logger";
import { Caller, ParsePathInteger, RealReply, RecordProgressionEvent } from "./progressionevents";
import { EnsureLoadoutBaseline, RecordLoadoutVersion, Tx } from "./savehistory";

const DEFAULT_INSTANCE_DATA = JSON.stringify({
    SheenType: 73,
    IsPrimarySheenActive: true,
    PrimaryDyeId: "None",
    IsSecondarySheenActive: true,
    SecondaryDyeId: "None",
    IsTertiarySheenActive: false,
    TertiaryDyeId: "None",
    TransmogCatalogId: "None",
    TransmogEnabled: false,
    EquippedCells: [],
    EquippedCellsv2: [],
    SubTypeMetadataArray: {
        ItemSubType: "subtype_eblade",
        EquippedItemParts: [{
            WeaponPartId: "PART_EB_SPECIAL_DEFAULT",
            SlotIndex: 0,
        }]
    }
});

const DEFAULT_PERSISTENT = {
    manual_emotes: ["", "", "", "", "", ""],
    intro_emote: "EM_INTRO_BEGINNER_01",
    banner: "BN_BEGINNER_00",
    bannerCustomization: "{\"BannerMeshItemID\":\"BNC_MESH_BEGINNER_00\",\"FabricMaterialItemID\":\"BNC_FABRIC_BEGINNER_00\",\"SigilTextureItemID\":\"BNC_SIGIL_BEGINNER_00\",\"PlantVFXItemID\":\"\",\"PersistantStandardVFXItemID\":\"\",\"AnimationItemID\":\"BNC_ANIMATION_BEGINNER_00\",\"BackgroundColourItemID\":\"DYE_BANNER_BACKGROUND_DEFAULT\",\"BorderColourItemID\":\"DYE_BANNER_SIGIL_DEFAULT\",\"SigilColourItemID\":\"DYE_BANNER_SIGIL_DEFAULT\",\"BackgroundSheenType\":0,\"BorderSheenType\":0,\"SigilSheenType\":0}",
    flare: "QI_BASIC_FLARE_DURABLE",
    title: "",
    head_accessory: "",
    back_accessory: "",
    pet: "",
    glider: "GD_FRAME_STARTER_BASE",
    update_version: 0,
    quick_chats: ["", "", "", "", "", "", "", "", ""],
    emojis: ["", "", "", "", "", "", "", "", ""],
    quick_curiosities_items: Array.from({length: 8}, (_, item_index) => ({
        item_index,
        item_id: "",
        instance_id: "",
    })),
    quickwheel: [],
};

function MakeDefaultLoadoutSlot(SlotIndex: number){
    return {
        weapon: {
            item_id: "WP_EB_TRAINING",
            instance_id: "WP_EB_TRAINING",
            instance_data: DEFAULT_INSTANCE_DATA
        },
        helmet: {
            item_id: "AR_UNEQUIPPED_HELM",
            instance_id: "AR_UNEQUIPPED_HELM",
            instance_data: DEFAULT_INSTANCE_DATA
        },
        chest: {
            item_id: "AR_BEGINNER_CHEST",
            instance_id: "AR_BEGINNER_CHEST",
            instance_data: DEFAULT_INSTANCE_DATA
        },
        arms: {
            item_id: "AR_BEGINNER_ARMS",
            instance_id: "AR_BEGINNER_ARMS",
            instance_data: DEFAULT_INSTANCE_DATA
        },
        legs: {
            item_id: "AR_BEGINNER_LEGS",
            instance_id: "AR_BEGINNER_LEGS",
            instance_data: DEFAULT_INSTANCE_DATA
        },
        lantern: {
            item_id: "LT_BASIC",
            instance_id: "LT_BASIC",
            instance_data: DEFAULT_INSTANCE_DATA
        },
        player_role: {
            item_id: "PR_DARKNESS",
            instance_id: "PR_DARKNESS",
            instance_data: DEFAULT_INSTANCE_DATA
        },
        subweapon: null,
        appearance: "{\"CreationState\":\"EArchonCharacterCreationState::NewCharacter\",\"Data\":[],\"AssetReferences\":[],\"StringData\":[]}",
        flask: "FL_HEALING_DEFAULT",
        quick_items: [],
        slot_index: SlotIndex,
        update_version: 0,
        custom_name: "",
        persistent: DEFAULT_PERSISTENT
    };
}

// A loadout save can race the initial /all read during login. Keep every entry point
// self-contained instead of requiring callers to create the row in a particular order.
function EnsureLoadoutRow(UserId: string, CharacterId: string){
    let Row = GetDb().select().from(loadouts)
        .where(and(eq(loadouts.characterId, CharacterId), eq(loadouts.userId, UserId))).get();

    if(Row !== undefined){
        return Row;
    }

    logger.info(`Creating new loadout set for userId ${UserId} and characterId ${CharacterId}`);
    GetDb().insert(loadouts).values({
        characterId: CharacterId,
        userId: UserId,
        loadouts: JSON.stringify([MakeDefaultLoadoutSlot(0)]),
        persistent: JSON.stringify(DEFAULT_PERSISTENT)
    }).onConflictDoNothing().run();

    Row = GetDb().select().from(loadouts)
        .where(and(eq(loadouts.characterId, CharacterId), eq(loadouts.userId, UserId))).get();

    if(Row === undefined){
        throw new Error(`Failed to create loadouts for userId ${UserId} and characterId ${CharacterId}`);
    }

    return Row;
}

export async function GetAllLoadoutsForUserIdAndCharacterId(UserId: string, CharacterId: string){
    return JSON.parse(EnsureLoadoutRow(UserId, CharacterId).loadouts);
}

export async function GetPersistentLoadoutForUserIdAndCharacterId(UserId: string, CharacterId: string){
    return JSON.parse(EnsureLoadoutRow(UserId, CharacterId).persistent);
}

// A save that matches no row, or whose data isn't JSON, fails instead of being
// dropped (or stored and then breaking every later GET of the loadouts).
export function SetLoadoutDataForUserIdAndCharacterId(UserId: string, CharacterId: string, Index: string, Data: string, Reason: string = "save"){
    logger.info(`Attempting to update loadout data Index ${Index}`);

    if(Index !== "0" && Index !== "persistent"){
        logger.error(`Unsupported Loadout Data Index ${Index}`);
        return false;
    }

    try{
        if(typeof Data !== "string"){
            throw new Error(`data is a ${Data === null ? "null" : typeof Data}, not a string`);
        }

        JSON.parse(Data);
    }
    catch(error){
        logger.error(`Refusing loadout index ${Index} for userId ${UserId} and characterId ${CharacterId}: ${(error as Error).message}`);
        return false;
    }

    try{
        return WriteLoadoutData(UserId, CharacterId, Index, Data, Reason);
    }
    catch(error){
        logger.error(error, `Failed to update loadout index ${Index} for userId ${UserId} and characterId ${CharacterId}`);
        return false;
    }
}

// Loadout slots of real-mode accounts (roadmap 2.4). The game server asks for
// POST .../unlock/{n} with n = slots to ADD (1 + unlock conditions met - slots it
// has), so the stored count only grows and the account + character total never
// passes the client's six-slot ceiling.
// The four counts are stack memory in the client and are never zeroed, so every
// reply carries all four as numbers, and max_character_slots is never below
// num_character_slots (the loadout carousel would memmove a negative length).
export const NUM_ACCOUNT_SLOTS = 1;
export const MAX_ACCOUNT_SLOTS = 1;
// The 1.4.4 client has six slots total. One is account-wide, leaving at most five
// character slots; advertising six here made the combined count seven.
export const MAX_CHARACTER_SLOTS = 5;

export type LoadoutSlots = { NumCharacterSlots: number, ActiveIndex: number };

export function GetLoadoutSlots(CharacterId: string): LoadoutSlots{
    const Row = GetDb().select().from(loadoutslots).where(eq(loadoutslots.characterId, CharacterId)).get();

    return {NumCharacterSlots: Row?.numCharacterSlots ?? 1, ActiveIndex: Row?.activeIndex ?? 0};
}

export function SlotCounts(Slots: LoadoutSlots){
    return {
        num_account_slots: NUM_ACCOUNT_SLOTS,
        max_account_slots: MAX_ACCOUNT_SLOTS,
        num_character_slots: Slots.NumCharacterSlots,
        max_character_slots: Math.max(MAX_CHARACTER_SLOTS, Slots.NumCharacterSlots)
    };
}

export function TotalLoadoutSlots(Slots: LoadoutSlots){
    return NUM_ACCOUNT_SLOTS + Slots.NumCharacterSlots;
}

function SlotIndexOf(Element: any){
    const Index = Number(Element?.slot_index ?? 0);

    return Number.isInteger(Index) ? Index : 0;
}

// Payload of GET .../all for a real-mode account: the stored counts and active slot,
// and only the elements of slots that exist, by slot index
export function RealLoadoutPayload(CharacterId: string, Loadouts: any[], Persistent: any){
    const Slots = GetLoadoutSlots(CharacterId);
    const Total = TotalLoadoutSlots(Slots);

    return {
        loadouts: Loadouts.filter((Element) => SlotIndexOf(Element) < Total).sort((A, B) => SlotIndexOf(A) - SlotIndexOf(B)),
        persistent: Persistent,
        ...SlotCounts(Slots),
        active_index: Slots.ActiveIndex < Total ? Slots.ActiveIndex : 0,
        needs_migration: false
    };
}

function WriteSlots(tx: Tx, UserId: string, CharacterId: string, Slots: LoadoutSlots){
    const Now = new Date().toISOString();

    tx.insert(loadoutslots).values({characterId: CharacterId, userId: UserId, numCharacterSlots: Slots.NumCharacterSlots, activeIndex: Slots.ActiveIndex, updatedDate: Now})
        .onConflictDoUpdate({target: loadoutslots.characterId, set: {numCharacterSlots: Slots.NumCharacterSlots, activeIndex: Slots.ActiveIndex, updatedDate: Now}}).run();
}

function ReadSlots(tx: Tx, CharacterId: string): LoadoutSlots{
    const Row = tx.select().from(loadoutslots).where(eq(loadoutslots.characterId, CharacterId)).get();

    return {NumCharacterSlots: Row?.numCharacterSlots ?? 1, ActiveIndex: Row?.activeIndex ?? 0};
}

// POST /loadout/:uid/:cid/unlock/:n (no body). Reply = stored + n, capped at 5
// character slots (plus the one account slot).
export function UnlockLoadoutSlots(UserId: string, CharacterId: string, NumSlotsRaw: string, Who: Caller): RealReply{
    const Route = "POST /loadout/:uid/:cid/unlock/:n";
    const NumSlots = ParsePathInteger(NumSlotsRaw);

    return GetDb().transaction((tx) => {
        if(NumSlots === undefined || NumSlots < 1){
            RecordProgressionEvent(tx, {AccountId: UserId, Caller: Who, Route, Status: 400, Notes: [`characterId ${CharacterId}`, `num_slots ${NumSlotsRaw} is not a positive integer`]});
            logger.warn(`Refusing loadout unlock of ${NumSlotsRaw} slot(s) for characterId ${CharacterId}: not a positive integer`);
            return {Status: 400};
        }

        const Current = ReadSlots(tx, CharacterId);
        const Wanted = Current.NumCharacterSlots + NumSlots;
        const Updated = {NumCharacterSlots: Math.min(Wanted, MAX_CHARACTER_SLOTS), ActiveIndex: Current.ActiveIndex};

        WriteSlots(tx, UserId, CharacterId, Updated);

        const Reply = {code: null, message: "OK", payload: SlotCounts(Updated)};
        const Notes = [`characterId ${CharacterId}`, `${Current.NumCharacterSlots} + ${NumSlots} -> ${Updated.NumCharacterSlots}`];

        if(Wanted > MAX_CHARACTER_SLOTS){
            Notes.push(`capped at ${MAX_CHARACTER_SLOTS}`);
        }

        RecordProgressionEvent(tx, {AccountId: UserId, Caller: Who, Route, Status: 200, Reply, Notes});
        logger.info(`Unlocked loadout slots for characterId ${CharacterId} of ${UserId}: ${Current.NumCharacterSlots} + ${NumSlots} -> ${Updated.NumCharacterSlots} character slot(s)`);

        return {Status: 200, Body: Reply};
    });
}

// GET /loadout/:uid/:cid/slotcount
export function GetSlotCountReply(CharacterId: string){
    return {code: null, message: "OK", payload: SlotCounts(GetLoadoutSlots(CharacterId))};
}

// POST /loadout/:uid/:cid/active/:index (no body)
export function SetActiveLoadoutSlot(UserId: string, CharacterId: string, IndexRaw: string, Who: Caller): RealReply{
    const Route = "POST /loadout/:uid/:cid/active/:index";
    const Index = ParsePathInteger(IndexRaw);

    return GetDb().transaction((tx) => {
        const Current = ReadSlots(tx, CharacterId);

        if(Index === undefined || Index >= TotalLoadoutSlots(Current)){
            RecordProgressionEvent(tx, {AccountId: UserId, Caller: Who, Route, Status: 400, Notes: [`characterId ${CharacterId}`, `index ${IndexRaw} is not a slot (${TotalLoadoutSlots(Current)} slots)`]});
            logger.warn(`Refusing active loadout slot ${IndexRaw} for characterId ${CharacterId}: ${TotalLoadoutSlots(Current)} slot(s)`);
            return {Status: 400};
        }

        WriteSlots(tx, UserId, CharacterId, {NumCharacterSlots: Current.NumCharacterSlots, ActiveIndex: Index});

        const Reply = {code: null, message: "OK"};

        RecordProgressionEvent(tx, {AccountId: UserId, Caller: Who, Route, Status: 200, Reply, Notes: [`characterId ${CharacterId}`, `active ${Current.ActiveIndex} -> ${Index}`]});
        logger.info(`Active loadout slot of characterId ${CharacterId} is now ${Index}`);

        return {Status: 200, Body: Reply};
    });
}

// Real-mode save of POST /loadout/:uid/:cid/:index: any slot that exists, stored by
// its slot_index (the element's own slot_index is set to the URL index)
export function SetLoadoutSlotData(UserId: string, CharacterId: string, Index: string, Data: string, Reason: string = "save"){
    if(Index === "persistent"){
        EnsureLoadoutRow(UserId, CharacterId);
        return SetLoadoutDataForUserIdAndCharacterId(UserId, CharacterId, Index, Data, Reason);
    }

    const SlotIndex = ParsePathInteger(Index);
    const Total = TotalLoadoutSlots(GetLoadoutSlots(CharacterId));

    if(SlotIndex === undefined || SlotIndex >= Total){
        logger.error(`Refusing loadout index ${Index} for userId ${UserId} and characterId ${CharacterId}: ${Total} slot(s) unlocked`);
        return false;
    }

    let Element: any;

    try{
        if(typeof Data !== "string"){
            throw new Error(`data is a ${Data === null ? "null" : typeof Data}, not a string`);
        }

        Element = JSON.parse(Data);

        if(Element == null || typeof Element !== "object" || Array.isArray(Element)){
            throw new Error("data is not a JSON object");
        }
    }
    catch(error){
        logger.error(`Refusing loadout index ${Index} for userId ${UserId} and characterId ${CharacterId}: ${(error as Error).message}`);
        return false;
    }

    if(Element.slot_index !== undefined && Element.slot_index !== SlotIndex){
        logger.warn(`Loadout for slot ${SlotIndex} of characterId ${CharacterId} carried slot_index ${JSON.stringify(Element.slot_index)}; stored as ${SlotIndex}`);
    }

    Element.slot_index = SlotIndex;

    try{
        EnsureLoadoutRow(UserId, CharacterId);

        return GetDb().transaction((tx) => {
            const Current = tx.select().from(loadouts).where(and(eq(loadouts.characterId, CharacterId), eq(loadouts.userId, UserId))).get();

            if(Current == undefined){
                logger.error(`Refusing loadout index ${Index} for userId ${UserId} and characterId ${CharacterId}: no loadout row for this character and user`);
                return false;
            }

            EnsureLoadoutBaseline(tx, Current);

            const Elements: any[] = JSON.parse(Current.loadouts);
            const Position = Elements.findIndex((Existing) => SlotIndexOf(Existing) === SlotIndex);

            if(Position >= 0){
                Elements[Position] = Element;
            }
            else{
                Elements.push(Element);
            }

            Elements.sort((A, B) => SlotIndexOf(A) - SlotIndexOf(B));

            const Updated = tx.update(loadouts).set({loadouts: JSON.stringify(Elements)})
                .where(and(eq(loadouts.characterId, CharacterId), eq(loadouts.userId, UserId))).returning().get();

            RecordLoadoutVersion(tx, Updated!, Reason);

            return true;
        });
    }
    catch(error){
        logger.error(error, `Failed to update loadout index ${Index} for userId ${UserId} and characterId ${CharacterId}`);
        return false;
    }
}

function WriteLoadoutData(UserId: string, CharacterId: string, Index: string, Data: string, Reason: string){
    return GetDb().transaction((tx) => {
        const Current = tx.select().from(loadouts).where(and(eq(loadouts.characterId, CharacterId), eq(loadouts.userId, UserId))).get();

        if(Current == undefined){
            logger.error(`Refusing loadout index ${Index} for userId ${UserId} and characterId ${CharacterId}: no loadout row for this character and user`);
            return false;
        }

        EnsureLoadoutBaseline(tx, Current);

        const Updated = Index === "0"
            ? tx.update(loadouts).set({
                // TODO: For multi-loadouts, validate the loadout index first. SQLite json_set
                // appends at array length and no-ops past it: https://www.sqlite.org/json1.html#jins
                // sql`json_set(${loadouts.loadouts}, ${`$[${LoadoutIndex}]`}, json(${Data}))`
                loadouts: sql`json_set(${loadouts.loadouts}, '$[0]', json(${Data}))`
            }).where(and(eq(loadouts.characterId, CharacterId), eq(loadouts.userId, UserId))).returning().get()
            : tx.update(loadouts).set({
                persistent: Data
            }).where(and(eq(loadouts.characterId, CharacterId), eq(loadouts.userId, UserId))).returning().get();

        if(Updated == undefined){
            logger.error(`Refusing loadout index ${Index} for userId ${UserId} and characterId ${CharacterId}: the update matched no row`);
            return false;
        }

        RecordLoadoutVersion(tx, Updated, Reason);

        return true;
    });
}
