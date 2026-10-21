//! Account bans and device signals (`controllers/moderation.ts`, `controllers/devicesignals.ts`):
//! what an admin sees of an account, banning it (optionally its address and its device), and the
//! launcher's device claims. Ban enforcement itself is in `auth.rs`.
//!
//! Device signals are client claims, not attestation: they are never used to ban automatically.

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::activity::State as Activity;
use crate::schema::{account_bans, account_device_signals, device_ban_signals, moderation_events, users};

pub type Conn = store::Connection;

/// A device claim counts for a device ban for a day.
const FRESH_MS: i64 = 24 * 60 * 60 * 1000;
const MAX_LOG_OFFSET: f64 = 1_000_000.0;

/// What the moderation routes refuse with: the `error` code of their 400.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Code(&'static str),
    Db(String),
}

impl From<diesel::result::Error> for Refusal {
    fn from(error: diesel::result::Error) -> Self {
        Self::Db(error.to_string())
    }
}

// ---- Addresses ----

/// `NormalizeAddress` on text: an IPv4-mapped address unwrapped, an IP in lower case as written.
pub fn normalize_address(text: &str) -> Option<String> {
    let plain = text
        .get(..7)
        .filter(|prefix| prefix.eq_ignore_ascii_case("::ffff:"))
        .map(|_| &text[7..])
        .filter(|rest| rest.parse::<std::net::Ipv4Addr>().is_ok() && is_dotted_quad(rest))
        .unwrap_or(text);
    plain
        .parse::<std::net::IpAddr>()
        .ok()
        .filter(|_| !plain.contains('%'))
        .map(|_| plain.to_ascii_lowercase())
}

/// `\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}`.
fn is_dotted_quad(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| (1..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}

/// `IsLoopbackAddress`: 127.0.0.0/8 (also IPv4-mapped) and `::1`, as text.
pub fn is_loopback_text(text: &str) -> bool {
    let v4 = text
        .get(..7)
        .filter(|prefix| prefix.eq_ignore_ascii_case("::ffff:"))
        .map_or(text, |_| &text[7..]);
    (v4.starts_with("127.") && is_dotted_quad(v4)) || text == "::1"
}

// ---- Device signals ----

/// HMAC-SHA256.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| block.map(|b| b ^ byte);
    let inner = Sha256::new().chain_update(pad(0x36)).chain_update(message).finalize();
    Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// `DEVICE_SIGNAL_HMAC_KEY`: at least 64 hex digits, an even count.
pub fn signal_key(secret: Option<&str>) -> Option<Vec<u8>> {
    let secret = secret?;
    (secret.len() >= 64 && secret.len() % 2 == 0 && secret.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| unhex(secret))
        .flatten()
}

fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// What a device report comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reported {
    Stored,
    Blocked(String),
}

/// `ReportDeviceSignals`: `{smbiosUuid?, tpmEkSha256?}`, stored as keyed hashes in place of the
/// account's earlier claims. `Code("device_signals_unavailable")` without a valid key, else any
/// refusal is `invalid_device_signals`.
pub fn report_signals(
    conn: &mut Conn,
    key: Option<&[u8]>,
    account: &str,
    body: Option<&Value>,
    now: i64,
) -> Result<Reported, Refusal> {
    let invalid = Refusal::Code("invalid_device_signals");
    let Some(body) = body.and_then(Value::as_object) else {
        return Err(invalid);
    };
    if body.keys().any(|k| k != "smbiosUuid" && k != "tpmEkSha256") {
        return Err(invalid);
    }
    let Some(key) = key else {
        return Err(Refusal::Code("device_signals_unavailable"));
    };
    let mut signals = Vec::new();
    for (kind, value) in body {
        let Some(value) = value.as_str() else {
            return Err(invalid);
        };
        let normalized = value.to_lowercase();
        let valid = if kind == "smbiosUuid" {
            is_uuid(&normalized)
        } else {
            normalized.len() == 64 && normalized.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        };
        let bare = normalized.replace('-', "");
        let blank = !bare.is_empty() && (bare.bytes().all(|b| b == b'0') || bare.bytes().all(|b| b == b'f'));
        if !valid || blank || normalized == "03000200-0400-0500-0006-000700080009" {
            return Err(invalid);
        }
        let hash = hex(&hmac_sha256(key, format!("{kind}\0{normalized}").as_bytes()));
        signals.push((kind.clone(), hash));
    }
    conn.transaction(|conn| {
        let exists = users::table
            .find(account)
            .select(users::userId)
            .first::<String>(conn)
            .optional()?
            .is_some();
        if !exists {
            return Err(Refusal::Code("account_not_found"));
        }
        diesel::delete(account_device_signals::table.filter(account_device_signals::account_id.eq(account)))
            .execute(conn)?;
        for (kind, hash) in &signals {
            diesel::insert_into(account_device_signals::table)
                .values((
                    account_device_signals::account_id.eq(account),
                    account_device_signals::kind.eq(kind),
                    account_device_signals::signal_hash.eq(hash),
                    account_device_signals::observed_at.eq(now),
                ))
                .execute(conn)?;
        }
        for (kind, hash) in &signals {
            let ban: Option<String> = device_ban_signals::table
                .inner_join(account_bans::table.on(account_bans::account_id.eq(device_ban_signals::account_id)))
                .filter(device_ban_signals::kind.eq(kind))
                .filter(device_ban_signals::signal_hash.eq(hash))
                .filter(account_bans::active.eq(1))
                .filter(account_bans::expires_at.is_null().or(account_bans::expires_at.gt(now)))
                .select(account_bans::reason)
                .first(conn)
                .optional()?;
            if let Some(reason) = ban {
                return Ok(Reported::Blocked(reason));
            }
        }
        Ok(Reported::Stored)
    })
}

/// `SnapshotDeviceBan`: the account's fresh device claims become its device ban (or it has none).
fn snapshot_device_ban(conn: &mut Conn, account: &str, enabled: bool, now: i64) -> Result<(), Refusal> {
    let signals: Vec<(String, String)> = if enabled {
        account_device_signals::table
            .filter(account_device_signals::account_id.eq(account))
            .filter(account_device_signals::observed_at.gt(now - FRESH_MS))
            .filter(account_device_signals::observed_at.le(now))
            .select((account_device_signals::kind, account_device_signals::signal_hash))
            .load(conn)?
    } else {
        Vec::new()
    };
    if enabled && signals.is_empty() {
        return Err(Refusal::Code("device_not_observed"));
    }
    diesel::delete(device_ban_signals::table.filter(device_ban_signals::account_id.eq(account))).execute(conn)?;
    for (kind, hash) in signals {
        diesel::insert_into(device_ban_signals::table)
            .values((
                device_ban_signals::account_id.eq(account),
                device_ban_signals::kind.eq(kind),
                device_ban_signals::signal_hash.eq(hash),
            ))
            .execute(conn)?;
    }
    Ok(())
}

/// `DeviceSignalInfo`.
fn device_info(conn: &mut Conn, account: &str, now: i64) -> QueryResult<Value> {
    let rows: Vec<(String, String, i64)> = account_device_signals::table
        .filter(account_device_signals::account_id.eq(account))
        .order(account_device_signals::kind)
        .select((
            account_device_signals::kind,
            account_device_signals::signal_hash,
            account_device_signals::observed_at,
        ))
        .load(conn)?;
    let enabled = device_ban_signals::table
        .filter(device_ban_signals::account_id.eq(account))
        .select(device_ban_signals::kind)
        .first::<String>(conn)
        .optional()?
        .is_some();
    let signals: Vec<Value> = rows
        .into_iter()
        .map(|(kind, hash, at)| {
            json!({
                "kind": kind,
                "hash": hash.chars().take(12).collect::<String>(),
                "observedAt": at,
                "fresh": at > now - FRESH_MS && at <= now,
            })
        })
        .collect();
    Ok(json!({"claimed": true, "verified": false, "enabled": enabled, "signals": signals}))
}

// ---- Bans ----

#[derive(QueryableByName)]
struct JsonRow {
    #[diesel(sql_type = Text)]
    j: String,
}

fn json_rows(rows: Vec<JsonRow>) -> Vec<Value> {
    rows.into_iter()
        .map(|row| serde_json::from_str(&row.j).unwrap_or(Value::Null))
        .collect()
}

/// `ModerationInfo`: what an admin sees of an account (its playing address only while it plays).
pub fn info(conn: &mut Conn, activity: &mut Activity, account: &str, now: i64) -> QueryResult<Value> {
    let address = activity.playing_address(account, now);
    let name: Option<String> = users::table.find(account).select(users::name).first(conn).optional()?;
    let ban = diesel::sql_query(
        "SELECT json_object('reason', reason, 'address', address, 'active', CASE WHEN expires_at <= ? THEN 0 ELSE active END, \
         'updated_at', updated_at, 'expires_at', expires_at) AS j FROM account_bans WHERE account_id = ?",
    )
    .bind::<BigInt, _>(now)
    .bind::<Text, _>(account)
    .load::<JsonRow>(conn)?;
    let addresses: Vec<Value> = address
        .map(|(address, seen)| json!({"address": address, "last_seen": seen}))
        .into_iter()
        .collect();
    Ok(json!({
        "username": name,
        "ban": json_rows(ban).into_iter().next(),
        "devices": device_info(conn, account, now)?,
        "addresses": addresses,
    }))
}

/// `SetAccountBan` `{accountId, reason, active, address?, durationHours?, includeDevice?}`, then
/// the account's moderation info. Admin accounts can't be banned; an address must be the one the
/// account is playing from now, and a device ban needs a claim from the last day.
pub fn set_ban(
    conn: &mut Conn,
    activity: &mut Activity,
    body: Option<&Value>,
    actor: &str,
    now: i64,
) -> Result<Value, Refusal> {
    let Some(body) = body else {
        // `req.body.accountId` on no body throws
        return Err(Refusal::Code("moderation_failed"));
    };
    let get = |name: &str| body.get(name);
    let include_device = match get("includeDevice") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(Refusal::Code("invalid_device_scope")),
    };
    let (Some(account), Some(reason), Some(active)) = (
        get("accountId").and_then(Value::as_str),
        get("reason").and_then(Value::as_str),
        get("active").and_then(Value::as_bool),
    ) else {
        return Err(Refusal::Code("invalid_ban"));
    };
    let reason_trimmed = crate::js::trim(reason);
    if reason_trimmed.is_empty() || reason.encode_utf16().count() > 500 {
        return Err(Refusal::Code("invalid_ban"));
    }
    // A falsy address that is neither text nor null (0, false) bans no address, as in Node
    let ip = match get("address") {
        Some(Value::String(text)) if !text.is_empty() => match normalize_address(text) {
            Some(ip) if !is_loopback_text(&ip) => Some(ip),
            _ => return Err(Refusal::Code("invalid_address")),
        },
        other if crate::js::truthy(other) => return Err(Refusal::Code("invalid_address")),
        _ => None,
    };
    let hours = match get("durationHours") {
        None => None,
        Some(value) => match value.as_f64() {
            Some(h) if h.fract() == 0.0 && (1.0..=720.0).contains(&h) => Some(h as i64),
            _ => return Err(Refusal::Code("invalid_duration")),
        },
    };
    let expires_at = hours.filter(|_| active).map(|h| now + h * 3_600_000);
    conn.transaction(|conn| {
        let admin: Option<bool> = users::table
            .find(account)
            .select(users::isAdmin)
            .first(conn)
            .optional()?;
        match admin {
            None => return Err(Refusal::Code("account_not_found")),
            Some(true) => return Err(Refusal::Code("admin_account_protected")),
            Some(false) => {}
        }
        if let Some(ip) = &ip
            && activity.playing_address(account, now).map(|(a, _)| a).as_deref() != Some(ip.as_str())
        {
            return Err(Refusal::Code("address_not_observed"));
        }
        snapshot_device_ban(conn, account, active && include_device, now)?;
        let active = i64::from(active);
        diesel::insert_into(account_bans::table)
            .values((
                account_bans::account_id.eq(account),
                account_bans::reason.eq(reason_trimmed),
                account_bans::address.eq(&ip),
                account_bans::active.eq(active),
                account_bans::updated_at.eq(now),
                account_bans::actor.eq(actor),
                account_bans::expires_at.eq(expires_at),
            ))
            .on_conflict(account_bans::account_id)
            .do_update()
            .set((
                account_bans::reason.eq(reason_trimmed),
                account_bans::address.eq(&ip),
                account_bans::active.eq(active),
                account_bans::updated_at.eq(now),
                account_bans::actor.eq(actor),
                account_bans::expires_at.eq(expires_at),
            ))
            .execute(conn)?;
        diesel::insert_into(moderation_events::table)
            .values((
                moderation_events::account_id.eq(account),
                moderation_events::reason.eq(reason_trimmed),
                moderation_events::address.eq(&ip),
                moderation_events::active.eq(active),
                moderation_events::created_at.eq(now),
                moderation_events::actor.eq(actor),
                moderation_events::expires_at.eq(expires_at),
            ))
            .execute(conn)?;
        Ok(info(conn, activity, account, now)?)
    })
}

/// `ModerationLog`: the active bans, or every moderation event, newest first, 50 a page.
pub fn log(conn: &mut Conn, events: bool, offset: f64, now: i64) -> Result<Value, Refusal> {
    if offset.fract() != 0.0 || !(0.0..=MAX_LOG_OFFSET).contains(&offset) {
        return Err(Refusal::Code("invalid_offset"));
    }
    let offset = offset as i64;
    let rows = if events {
        diesel::sql_query(
            "SELECT json_object('id', e.id, 'account_id', e.account_id, 'username', u.name, 'reason', e.reason, \
             'active', e.active, 'created_at', e.created_at, 'expires_at', e.expires_at, 'actor', e.actor) AS j \
             FROM moderation_events e LEFT JOIN users u ON u.userId = e.account_id ORDER BY e.id DESC LIMIT 51 OFFSET ?",
        )
        .bind::<BigInt, _>(offset)
        .load::<JsonRow>(conn)?
    } else {
        diesel::sql_query(
            "SELECT json_object('account_id', b.account_id, 'username', u.name, 'reason', b.reason, \
             'updated_at', b.updated_at, 'expires_at', b.expires_at, 'actor', b.actor) AS j \
             FROM account_bans b LEFT JOIN users u ON u.userId = b.account_id \
             WHERE b.active = 1 AND (b.expires_at IS NULL OR b.expires_at > ?) \
             ORDER BY b.updated_at DESC, b.account_id LIMIT 51 OFFSET ?",
        )
        .bind::<BigInt, _>(now)
        .bind::<BigInt, _>(offset)
        .load::<JsonRow>(conn)?
    };
    let mut rows = json_rows(rows);
    let next = (rows.len() > 50).then_some(offset + 50);
    rows.truncate(50);
    Ok(json!({"rows": rows, "nextOffset": next}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc_4231() {
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            hex(&hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn addresses_are_read_like_node() {
        assert_eq!(normalize_address("::FFFF:1.2.3.4"), Some("1.2.3.4".into()));
        assert_eq!(normalize_address("2001:DB8::1"), Some("2001:db8::1".into()));
        assert_eq!(normalize_address("nope"), None);
        assert!(is_loopback_text("127.0.0.9"));
        assert!(is_loopback_text("::1"));
        assert!(!is_loopback_text("0:0:0:0:0:0:0:1"));
    }

    #[test]
    fn signal_keys_are_hex() {
        assert!(signal_key(Some(&"ab".repeat(32))).is_some());
        assert!(signal_key(Some(&"ab".repeat(31))).is_none());
        assert!(signal_key(Some(&format!("{}a", "ab".repeat(32)))).is_none());
        assert!(signal_key(None).is_none());
    }
}
