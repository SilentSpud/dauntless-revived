//! Loadouts (`controllers/loadout.ts`): the gear slots and the account-wide "persistent" loadout of a
//! character, and in real mode the loadout slots the game server unlocks.
//!
//! The stored text is what Node wrote: `JSON.stringify` output, or for slot 0 in stub mode SQLite's
//! own `json_set(…, json(data))`, which is why that write stays SQL here.

use diesel::prelude::*;
use diesel::sql_types::Text;
use serde_json::{Map, Value, json};
use service_kit::jsjson;

use crate::characters::{HistorySettings, LoadoutRow, ensure_loadout_baseline, record_loadout_version};
use crate::progression::{Event, parse_path_integer, record_event};
use crate::schema::{loadouts, loadoutslots};
use crate::time::iso_now;

pub type Conn = store::Connection;

pub const NUM_ACCOUNT_SLOTS: i64 = 1;
pub const MAX_ACCOUNT_SLOTS: i64 = 1;
/// The 1.4.4 client has six slots in all, one of them account-wide.
pub const MAX_CHARACTER_SLOTS: i64 = 5;

fn default_instance_data() -> String {
    jsjson::stringify(&json!({
        "SheenType": 73,
        "IsPrimarySheenActive": true,
        "PrimaryDyeId": "None",
        "IsSecondarySheenActive": true,
        "SecondaryDyeId": "None",
        "IsTertiarySheenActive": false,
        "TertiaryDyeId": "None",
        "TransmogCatalogId": "None",
        "TransmogEnabled": false,
        "EquippedCells": [],
        "EquippedCellsv2": [],
        "SubTypeMetadataArray": {
            "ItemSubType": "subtype_eblade",
            "EquippedItemParts": [{"WeaponPartId": "PART_EB_SPECIAL_DEFAULT", "SlotIndex": 0}],
        },
    }))
}

fn default_persistent() -> Value {
    json!({
        "manual_emotes": ["", "", "", "", "", ""],
        "intro_emote": "EM_INTRO_BEGINNER_01",
        "banner": "BN_BEGINNER_00",
        "bannerCustomization": "{\"BannerMeshItemID\":\"BNC_MESH_BEGINNER_00\",\"FabricMaterialItemID\":\"BNC_FABRIC_BEGINNER_00\",\"SigilTextureItemID\":\"BNC_SIGIL_BEGINNER_00\",\"PlantVFXItemID\":\"\",\"PersistantStandardVFXItemID\":\"\",\"AnimationItemID\":\"BNC_ANIMATION_BEGINNER_00\",\"BackgroundColourItemID\":\"DYE_BANNER_BACKGROUND_DEFAULT\",\"BorderColourItemID\":\"DYE_BANNER_SIGIL_DEFAULT\",\"SigilColourItemID\":\"DYE_BANNER_SIGIL_DEFAULT\",\"BackgroundSheenType\":0,\"BorderSheenType\":0,\"SigilSheenType\":0}",
        "flare": "QI_BASIC_FLARE_DURABLE",
        "title": "",
        "head_accessory": "",
        "back_accessory": "",
        "pet": "",
        "glider": "GD_FRAME_STARTER_BASE",
        "update_version": 0,
        "quick_chats": ["", "", "", "", "", "", "", "", ""],
        "emojis": ["", "", "", "", "", "", "", "", ""],
        "quick_curiosities_items": (0..8).map(|index| json!({"item_index": index, "item_id": "", "instance_id": ""})).collect::<Vec<_>>(),
        "quickwheel": [],
    })
}

fn default_slot(slot_index: i64) -> Value {
    let instance = default_instance_data();
    let item = |id: &str| json!({"item_id": id, "instance_id": id, "instance_data": instance});
    json!({
        "weapon": item("WP_EB_TRAINING"),
        "helmet": item("AR_UNEQUIPPED_HELM"),
        "chest": item("AR_BEGINNER_CHEST"),
        "arms": item("AR_BEGINNER_ARMS"),
        "legs": item("AR_BEGINNER_LEGS"),
        "lantern": item("LT_BASIC"),
        "player_role": item("PR_DARKNESS"),
        "subweapon": null,
        "appearance": "{\"CreationState\":\"EArchonCharacterCreationState::NewCharacter\",\"Data\":[],\"AssetReferences\":[],\"StringData\":[]}",
        "flask": "FL_HEALING_DEFAULT",
        "quick_items": [],
        "slot_index": slot_index,
        "update_version": 0,
        "custom_name": "",
        "persistent": default_persistent(),
    })
}

fn find_row(conn: &mut Conn, user_id: &str, character_id: &str) -> QueryResult<Option<LoadoutRow>> {
    loadouts::table
        .filter(loadouts::characterId.eq(character_id))
        .filter(loadouts::userId.eq(user_id))
        .select(LoadoutRow::as_select())
        .first(conn)
        .optional()
}

/// `EnsureLoadoutRow`: a character's loadouts, made with the defaults on first use.
pub fn ensure_row(conn: &mut Conn, user_id: &str, character_id: &str) -> QueryResult<LoadoutRow> {
    if let Some(row) = find_row(conn, user_id, character_id)? {
        return Ok(row);
    }
    tracing::info!("Creating new loadout set for userId {user_id} and characterId {character_id}");
    diesel::insert_into(loadouts::table)
        .values((
            loadouts::characterId.eq(character_id),
            loadouts::userId.eq(user_id),
            loadouts::loadouts_.eq(jsjson::stringify(&json!([default_slot(0)]))),
            loadouts::persistent.eq(jsjson::stringify(&default_persistent())),
        ))
        .on_conflict_do_nothing()
        .execute(conn)?;
    find_row(conn, user_id, character_id)?.ok_or(diesel::result::Error::NotFound)
}

/// `Number(value)` for what a stored `slot_index` can be.
fn js_number(value: &Value) -> f64 {
    match value {
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                0.0
            } else {
                text.parse().unwrap_or(f64::NAN)
            }
        }
        Value::Array(items) => match items.as_slice() {
            [] => 0.0,
            [one] if !one.is_array() && !one.is_object() => js_number(&Value::String(match one {
                Value::String(text) => text.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            })),
            _ => f64::NAN,
        },
        Value::Object(_) => f64::NAN,
    }
}

/// `SlotIndexOf`: `Number(slot_index ?? 0)`, 0 when not an integer.
fn slot_index_of(element: &Value) -> f64 {
    let index = js_number(element.get("slot_index").unwrap_or(&Value::Null));
    if index.is_finite() && index.fract() == 0.0 {
        index
    } else {
        0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slots {
    pub character: i64,
    pub active: i64,
}

pub fn read_slots(conn: &mut Conn, character_id: &str) -> QueryResult<Slots> {
    let row: Option<(i64, i64)> = loadoutslots::table
        .find(character_id)
        .select((loadoutslots::numCharacterSlots, loadoutslots::activeIndex))
        .first(conn)
        .optional()?;
    Ok(row.map_or(
        Slots {
            character: 1,
            active: 0,
        },
        |(character, active)| Slots { character, active },
    ))
}

fn write_slots(conn: &mut Conn, user_id: &str, character_id: &str, slots: Slots) -> QueryResult<()> {
    let now = iso_now();
    diesel::insert_into(loadoutslots::table)
        .values((
            loadoutslots::characterId.eq(character_id),
            loadoutslots::userId.eq(user_id),
            loadoutslots::numCharacterSlots.eq(slots.character),
            loadoutslots::activeIndex.eq(slots.active),
            loadoutslots::updatedDate.eq(&now),
        ))
        .on_conflict(loadoutslots::characterId)
        .do_update()
        .set((
            loadoutslots::numCharacterSlots.eq(slots.character),
            loadoutslots::activeIndex.eq(slots.active),
            loadoutslots::updatedDate.eq(&now),
        ))
        .execute(conn)
        .map(drop)
}

fn total(slots: Slots) -> i64 {
    NUM_ACCOUNT_SLOTS + slots.character
}

/// `SlotCounts`: all four counts, and `max_character_slots` never below `num_character_slots`.
pub fn slot_counts(slots: Slots) -> Value {
    json!({
        "num_account_slots": NUM_ACCOUNT_SLOTS,
        "max_account_slots": MAX_ACCOUNT_SLOTS,
        "num_character_slots": slots.character,
        "max_character_slots": MAX_CHARACTER_SLOTS.max(slots.character),
    })
}

/// The reply of GET …/all and of a save: real mode only shows slots that exist, by slot index.
pub fn payload(conn: &mut Conn, user_id: &str, character_id: &str, real: bool) -> Result<Value, crate::app::AppError> {
    let row = ensure_row(conn, user_id, character_id)?;
    let parse = |text: &str| {
        jsjson::parse(text).map_err(|e| crate::app::AppError::Other(format!("stored loadouts are not JSON: {e}")))
    };
    let loadouts = parse(&row.loadouts_)?;
    let persistent = parse(&row.persistent)?;
    let count = loadouts.as_array().map_or(0, Vec::len);
    tracing::info!("Fetched {count} loadout(s) for userId {user_id} and characterId {character_id}");
    if !real {
        return Ok(json!({
            "loadouts": loadouts,
            "persistent": persistent,
            "num_account_slots": 1,
            "max_account_slots": 1,
            "num_character_slots": 1,
            "max_character_slots": 1,
            "active_index": 0,
            "needs_migration": false,
        }));
    }
    let slots = read_slots(conn, character_id)?;
    let total = total(slots) as f64;
    // .filter(...).sort(...) in JavaScript: a non-array has no filter, a 500 as in Node.
    let Value::Array(items) = loadouts else {
        return Err(crate::app::AppError::Other("stored loadouts are not a list".into()));
    };
    let mut shown: Vec<Value> = items
        .into_iter()
        .filter(|element| slot_index_of(element) < total)
        .collect();
    shown.sort_by(|a, b| {
        slot_index_of(a)
            .partial_cmp(&slot_index_of(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut reply = Map::new();
    reply.insert("loadouts".into(), Value::Array(shown));
    reply.insert("persistent".into(), persistent);
    if let Value::Object(counts) = slot_counts(slots) {
        reply.extend(counts);
    }
    reply.insert(
        "active_index".into(),
        json!(if (slots.active as f64) < total { slots.active } else { 0 }),
    );
    reply.insert("needs_migration".into(), json!(false));
    Ok(Value::Object(reply))
}

/// A real-mode reply: status and optional JSON body.
pub type RealReply = (u16, Option<Value>);

/// `UnlockLoadoutSlots`: n MORE slots, capped at five character slots.
pub fn unlock(conn: &mut Conn, user_id: &str, character_id: &str, raw: &str, caller: &str) -> QueryResult<RealReply> {
    let route = "POST /loadout/:uid/:cid/unlock/:n";
    let Some(n) = parse_path_integer(raw, false).filter(|n| *n >= 1) else {
        let notes = [
            format!("characterId {character_id}"),
            format!("num_slots {raw} is not a positive integer"),
        ];
        record_event(
            conn,
            Event {
                account: user_id,
                caller,
                route,
                body: None,
                status: 400,
                reply: None,
                notes: &notes,
            },
        )?;
        tracing::warn!(
            "Refusing loadout unlock of {raw} slot(s) for characterId {character_id}: not a positive integer"
        );
        return Ok((400, None));
    };
    let current = read_slots(conn, character_id)?;
    let wanted = current.character + n;
    let updated = Slots {
        character: wanted.min(MAX_CHARACTER_SLOTS),
        active: current.active,
    };
    write_slots(conn, user_id, character_id, updated)?;
    let reply = json!({"code": null, "message": "OK", "payload": slot_counts(updated)});
    let mut notes = vec![
        format!("characterId {character_id}"),
        format!("{} + {n} -> {}", current.character, updated.character),
    ];
    if wanted > MAX_CHARACTER_SLOTS {
        notes.push(format!("capped at {MAX_CHARACTER_SLOTS}"));
    }
    record_event(
        conn,
        Event {
            account: user_id,
            caller,
            route,
            body: None,
            status: 200,
            reply: Some(&reply),
            notes: &notes,
        },
    )?;
    tracing::info!(
        "Unlocked loadout slots for characterId {character_id} of {user_id}: {} + {n} -> {} character slot(s)",
        current.character,
        updated.character
    );
    Ok((200, Some(reply)))
}

/// `SetActiveLoadoutSlot`.
pub fn set_active(
    conn: &mut Conn,
    user_id: &str,
    character_id: &str,
    raw: &str,
    caller: &str,
) -> QueryResult<RealReply> {
    let route = "POST /loadout/:uid/:cid/active/:index";
    let current = read_slots(conn, character_id)?;
    let total = total(current);
    let Some(index) = parse_path_integer(raw, false).filter(|index| *index < total) else {
        let notes = [
            format!("characterId {character_id}"),
            format!("index {raw} is not a slot ({total} slots)"),
        ];
        record_event(
            conn,
            Event {
                account: user_id,
                caller,
                route,
                body: None,
                status: 400,
                reply: None,
                notes: &notes,
            },
        )?;
        tracing::warn!("Refusing active loadout slot {raw} for characterId {character_id}: {total} slot(s)");
        return Ok((400, None));
    };
    write_slots(
        conn,
        user_id,
        character_id,
        Slots {
            character: current.character,
            active: index,
        },
    )?;
    let reply = json!({"code": null, "message": "OK"});
    let notes = [
        format!("characterId {character_id}"),
        format!("active {} -> {index}", current.active),
    ];
    record_event(
        conn,
        Event {
            account: user_id,
            caller,
            route,
            body: None,
            status: 200,
            reply: Some(&reply),
            notes: &notes,
        },
    )?;
    tracing::info!("Active loadout slot of characterId {character_id} is now {index}");
    Ok((200, Some(reply)))
}

fn kind(value: Option<&Value>) -> &'static str {
    match value {
        None => "undefined",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(_) => "object",
    }
}

/// `SetLoadoutDataForUserIdAndCharacterId` (stub mode, and real mode's persistent loadout): slot 0
/// through SQLite's `json_set`, or the persistent text as sent. False when refused.
pub fn set_data(
    conn: &mut Conn,
    history: &HistorySettings,
    user_id: &str,
    character_id: &str,
    index: &str,
    data: Option<&Value>,
    reason: &str,
) -> QueryResult<bool> {
    tracing::info!("Attempting to update loadout data Index {index}");
    if index != "0" && index != "persistent" {
        tracing::error!("Unsupported Loadout Data Index {index}");
        return Ok(false);
    }
    let text = match data {
        Some(Value::String(text)) => match jsjson::parse(text) {
            Ok(_) => text.clone(),
            Err(error) => {
                tracing::error!(
                    "Refusing loadout index {index} for userId {user_id} and characterId {character_id}: {error}"
                );
                return Ok(false);
            }
        },
        other => {
            tracing::error!(
                "Refusing loadout index {index} for userId {user_id} and characterId {character_id}: data is a {}, not a string",
                kind(other)
            );
            return Ok(false);
        }
    };
    let Some(current) = find_row(conn, user_id, character_id)? else {
        tracing::error!(
            "Refusing loadout index {index} for userId {user_id} and characterId {character_id}: no loadout row for this character and user"
        );
        return Ok(false);
    };
    ensure_loadout_baseline(conn, &current)?;
    let target = loadouts::table
        .filter(loadouts::characterId.eq(character_id))
        .filter(loadouts::userId.eq(user_id));
    let updated: Option<LoadoutRow> = if index == "0" {
        diesel::update(target)
            .set(
                loadouts::loadouts_.eq(diesel::dsl::sql::<Text>("json_set(loadouts, '$[0]', json(")
                    .bind::<Text, _>(text)
                    .sql("))")),
            )
            .returning(LoadoutRow::as_returning())
            .get_result(conn)
            .optional()?
    } else {
        diesel::update(target)
            .set(loadouts::persistent.eq(&text))
            .returning(LoadoutRow::as_returning())
            .get_result(conn)
            .optional()?
    };
    let Some(updated) = updated else {
        tracing::error!(
            "Refusing loadout index {index} for userId {user_id} and characterId {character_id}: the update matched no row"
        );
        return Ok(false);
    };
    record_loadout_version(conn, history, &updated, reason)?;
    Ok(true)
}

/// `SetLoadoutSlotData` (real mode): any unlocked slot, stored by its slot index.
pub fn set_slot(
    conn: &mut Conn,
    history: &HistorySettings,
    user_id: &str,
    character_id: &str,
    index: &str,
    data: Option<&Value>,
    reason: &str,
) -> QueryResult<bool> {
    if index == "persistent" {
        ensure_row(conn, user_id, character_id)?;
        return set_data(conn, history, user_id, character_id, index, data, reason);
    }
    let total = total(read_slots(conn, character_id)?);
    let Some(slot) = parse_path_integer(index, false).filter(|slot| *slot < total) else {
        tracing::error!(
            "Refusing loadout index {index} for userId {user_id} and characterId {character_id}: {total} slot(s) unlocked"
        );
        return Ok(false);
    };
    let mut element = match data {
        Some(Value::String(text)) => match jsjson::parse(text) {
            Ok(Value::Object(element)) => element,
            Ok(_) => {
                tracing::error!(
                    "Refusing loadout index {index} for userId {user_id} and characterId {character_id}: data is not a JSON object"
                );
                return Ok(false);
            }
            Err(error) => {
                tracing::error!(
                    "Refusing loadout index {index} for userId {user_id} and characterId {character_id}: {error}"
                );
                return Ok(false);
            }
        },
        other => {
            tracing::error!(
                "Refusing loadout index {index} for userId {user_id} and characterId {character_id}: data is a {}, not a string",
                kind(other)
            );
            return Ok(false);
        }
    };
    if let Some(carried) = element.get("slot_index")
        && carried.as_f64() != Some(slot as f64)
    {
        tracing::warn!(
            "Loadout for slot {slot} of characterId {character_id} carried slot_index {}; stored as {slot}",
            jsjson::stringify(carried)
        );
    }
    element.insert("slot_index".into(), json!(slot));
    ensure_row(conn, user_id, character_id)?;
    let Some(current) = find_row(conn, user_id, character_id)? else {
        tracing::error!(
            "Refusing loadout index {index} for userId {user_id} and characterId {character_id}: no loadout row for this character and user"
        );
        return Ok(false);
    };
    ensure_loadout_baseline(conn, &current)?;
    let Ok(Value::Array(mut elements)) = jsjson::parse(&current.loadouts_) else {
        tracing::error!(
            "Failed to update loadout index {index} for userId {user_id} and characterId {character_id}: the stored loadouts are not a list"
        );
        return Ok(false);
    };
    let element = Value::Object(element);
    match elements
        .iter()
        .position(|existing| slot_index_of(existing) == slot as f64)
    {
        Some(position) => elements[position] = element,
        None => elements.push(element),
    }
    elements.sort_by(|a, b| {
        slot_index_of(a)
            .partial_cmp(&slot_index_of(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let updated: LoadoutRow = diesel::update(
        loadouts::table
            .filter(loadouts::characterId.eq(character_id))
            .filter(loadouts::userId.eq(user_id)),
    )
    .set(loadouts::loadouts_.eq(jsjson::stringify(&Value::Array(elements))))
    .returning(LoadoutRow::as_returning())
    .get_result(conn)?;
    record_loadout_version(conn, history, &updated, reason)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_indexes_read_like_number() {
        for (value, expected) in [
            (json!(2), 2.0),
            (json!("3"), 3.0),
            (json!(" "), 0.0),
            (json!(true), 1.0),
            (json!(null), 0.0),
            (json!([4]), 4.0),
            (json!({}), 0.0),
            (json!(1.5), 0.0),
        ] {
            assert_eq!(slot_index_of(&json!({"slot_index": value})), expected, "{value}");
        }
        assert_eq!(slot_index_of(&json!({})), 0.0);
    }

    #[test]
    fn counts_never_shrink_the_maximum() {
        assert_eq!(
            slot_counts(Slots {
                character: 7,
                active: 0
            })["max_character_slots"],
            7
        );
        assert_eq!(
            slot_counts(Slots {
                character: 2,
                active: 0
            })["max_character_slots"],
            5
        );
    }
}
