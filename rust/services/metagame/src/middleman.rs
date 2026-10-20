//! The Middleman cell exchange's pending tokens in the inventory (`controllers/middleman.ts`): a
//! `TOKEN_CELL_EXCHANGE` item per exchange slot (1-3), stored under a slot-stable instance id so
//! several fusions survive at once, and the checks on finishing one.

use serde_json::{Map, Value};
use service_kit::jsjson;

use crate::js;

pub const FUSION_CATALOG_ID: &str = "TOKEN_CELL_EXCHANGE";

/// A refusal of the fusion rules: `Validation` is a 400, `Conflict` a 409.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FusionError {
    Validation(String),
    Conflict(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FusionData {
    pub slot_id: f64,
    pub end_time: String,
    pub result_cell: String,
    pub exchange_id: String,
}

fn invalid() -> FusionError {
    FusionError::Validation("Invalid Middleman fusion token".into())
}

/// `ParseFusionData`: a string `itemData` is parsed, anything else taken as it is.
pub fn parse_data(item: &Map<String, Value>) -> Result<FusionData, FusionError> {
    let data = match item.get("itemData") {
        Some(Value::String(text)) => jsjson::parse(text).map_err(|_| invalid())?,
        Some(other) => other.clone(),
        None => Value::Null,
    };
    let slot = data
        .get("SlotID")
        .and_then(Value::as_f64)
        .filter(|s| s.fract() == 0.0 && (1.0..=3.0).contains(s));
    let end = data
        .get("EndTime")
        .and_then(Value::as_str)
        .filter(|t| js::date_parse(t).is_some());
    let cell = data
        .get("ResultCell")
        .and_then(Value::as_str)
        .filter(|c| c.starts_with("CELL_"));
    let exchange = data.get("ExchangeID").and_then(Value::as_str).filter(|e| !e.is_empty());
    match (slot, end, cell, exchange) {
        (Some(slot_id), Some(end), Some(cell), Some(exchange)) => Ok(FusionData {
            slot_id,
            end_time: end.to_owned(),
            result_cell: cell.to_owned(),
            exchange_id: exchange.to_owned(),
        }),
        _ => Err(invalid()),
    }
}

fn try_parse(item: &Map<String, Value>) -> Option<FusionData> {
    parse_data(item).ok()
}

fn is_fusion(item: &Value) -> bool {
    item.get("catalogId").and_then(Value::as_str) == Some(FUSION_CATALOG_ID)
}

/// `NormaliseFusionItem`: a fusion token's instance id becomes `TOKEN_CELL_EXCHANGE:<slot>`. Strict
/// (an unreadable token is refused) for adds and saves, lenient for removals.
pub fn normalise(item: &mut Value, lenient: bool) -> Result<(), FusionError> {
    if !is_fusion(item) || js::is_nullish(item.get("itemData")) {
        return Ok(());
    }
    let Value::Object(map) = item else { return Ok(()) };
    let data = if lenient {
        try_parse(map)
    } else {
        Some(parse_data(map)?)
    };
    if let Some(data) = data {
        map.insert(
            "instanceId".into(),
            Value::String(format!("{FUSION_CATALOG_ID}:{}", jsjson::number(data.slot_id))),
        );
    }
    Ok(())
}

/// `FindFusionItemIndex`: the stored token of the same slot or exchange; or, for a client that still
/// sends the generic id, the only fusion token there is.
pub fn find_index(items: &[Value], incoming: &Value) -> Option<usize> {
    if !is_fusion(incoming) {
        return None;
    }
    if let Some(incoming_data) = incoming.as_object().and_then(try_parse) {
        let found = items.iter().position(|item| {
            is_fusion(item)
                && item.as_object().and_then(try_parse).is_some_and(|current| {
                    current.slot_id == incoming_data.slot_id || current.exchange_id == incoming_data.exchange_id
                })
        });
        if found.is_some() {
            return found;
        }
    }
    if incoming.get("instanceId").and_then(Value::as_str) == Some(FUSION_CATALOG_ID) {
        let fusions: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| is_fusion(item))
            .map(|(index, _)| index)
            .collect();
        if let [only] = fusions.as_slice() {
            return Some(*only);
        }
    }
    None
}

fn same_fusion(current: &Value, incoming: &Value) -> bool {
    if !is_fusion(current)
        || !is_fusion(incoming)
        || js::is_nullish(current.get("itemData"))
        || js::is_nullish(incoming.get("itemData"))
    {
        return false;
    }
    match (
        current.as_object().and_then(try_parse),
        incoming.as_object().and_then(try_parse),
    ) {
        (Some(c), Some(i)) => {
            c.slot_id == i.slot_id && c.exchange_id == i.exchange_id && c.result_cell == i.result_cell
        }
        _ => false,
    }
}

/// `IsValidFusionSave`: a speed-up moves `EndTime` earlier without a new version; nothing else may
/// change.
pub fn is_valid_save(current: &Value, incoming: &Value, operation: &str) -> Result<bool, FusionError> {
    if operation != "save" && operation != "update" {
        return Ok(false);
    }
    if !js::strict_eq(current.get("updateVersion"), incoming.get("updateVersion")) || !same_fusion(current, incoming) {
        return Ok(false);
    }
    let end = |item: &Value| -> Result<i64, FusionError> {
        let data = parse_data(item.as_object().ok_or_else(invalid)?)?;
        js::date_parse(&data.end_time).ok_or_else(invalid)
    };
    Ok(end(incoming)? <= end(current)?)
}

/// `IsValidFusionRemoval`: claiming a finished exchange removes the token, whatever version the client
/// holds.
pub fn is_valid_removal(current: &Value, incoming: &Value, operation: &str) -> bool {
    operation == "remove" && is_fusion(current) && is_fusion(incoming)
}

/// `ValidateFusionCompletions` (`MIDDLEMAN_FUSION_GUARDS`): a finished exchange may only be claimed
/// after its end time, granting exactly its result cell.
pub fn validate_completions(
    current: &[Value],
    removing: &[Value],
    adding_stacks: &[Value],
    now: i64,
) -> Result<(), FusionError> {
    for incoming in removing.iter().filter(|item| is_fusion(item)) {
        let Some(index) = find_index(current, incoming) else {
            return Err(FusionError::Conflict(
                "Middleman fusion token is no longer pending".into(),
            ));
        };
        let data = parse_data(current[index].as_object().ok_or_else(invalid)?)?;
        let slot = jsjson::number(data.slot_id);
        if js::date_parse(&data.end_time).is_some_and(|end| end > now) {
            return Err(FusionError::Conflict(format!(
                "Middleman fusion slot {slot} is not complete yet"
            )));
        }
        let reward = adding_stacks
            .iter()
            .find(|item| item.get("catalogId").and_then(Value::as_str) == Some(&data.result_cell));
        let quantity = js::number(Some(reward.and_then(|r| r.get("quantity")).unwrap_or(&Value::from(0))));
        if !(quantity.fract() == 0.0 && quantity.abs() <= 9_007_199_254_740_991.0 && quantity >= 1.0) {
            return Err(FusionError::Conflict(format!(
                "Middleman fusion slot {slot} did not grant its result cell {}",
                data.result_cell
            )));
        }
        if let Some(other) = adding_stacks.iter().find_map(|item| {
            item.get("catalogId")
                .and_then(Value::as_str)
                .filter(|id| id.starts_with("CELL_") && *id != data.result_cell)
        }) {
            return Err(FusionError::Conflict(format!(
                "Middleman fusion slot {slot} tried to grant {other} instead of {}",
                data.result_cell
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn token(slot: u8, exchange: &str, end: &str) -> Value {
        let data = json!({"SlotID": slot, "EndTime": end, "ResultCell": "CELL_A", "ExchangeID": exchange}).to_string();
        json!({"catalogId": FUSION_CATALOG_ID, "instanceId": FUSION_CATALOG_ID, "itemData": data, "updateVersion": 1})
    }

    #[test]
    fn tokens_get_slot_ids_and_match_by_slot_or_exchange() {
        let mut item = token(2, "x1", "2026-10-20T00:00:00Z");
        normalise(&mut item, false).unwrap();
        assert_eq!(item["instanceId"], "TOKEN_CELL_EXCHANGE:2");
        let stored = vec![json!({"catalogId": "CELL_B"}), item];
        assert_eq!(find_index(&stored, &token(3, "x1", "2026-10-20T00:00:00Z")), Some(1));
        let mut bad = json!({"catalogId": FUSION_CATALOG_ID, "itemData": "{\"SlotID\":0}"});
        assert!(normalise(&mut bad, false).is_err());
        assert!(normalise(&mut bad, true).is_ok());
    }

    #[test]
    fn completions_need_the_end_time_and_the_result_cell() {
        let stored = vec![token(1, "x", "2026-10-20T00:00:00Z")];
        let removing = vec![token(1, "x", "2026-10-20T00:00:00Z")];
        let before = 1_792_454_399_000;
        assert!(matches!(
            validate_completions(&stored, &removing, &[], before),
            Err(FusionError::Conflict(_))
        ));
        let after = 1_792_454_401_000;
        assert!(
            validate_completions(
                &stored,
                &removing,
                &[json!({"catalogId": "CELL_A", "quantity": 1})],
                after
            )
            .is_ok()
        );
        assert!(
            validate_completions(
                &stored,
                &removing,
                &[
                    json!({"catalogId": "CELL_A", "quantity": 1}),
                    json!({"catalogId": "CELL_B"})
                ],
                after
            )
            .is_err()
        );
    }
}
