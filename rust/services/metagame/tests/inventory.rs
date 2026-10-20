//! `SetDailyTokensOnce` (run at start, outside the parity scripts): every character's daily token
//! stacks are set to the daily amounts the first time, and never again.

use diesel::prelude::*;
use metagame::db::SCHEMA;
use metagame::inventory::{Settings, set_daily_tokens_once};
use store::{Options, Store};

#[tokio::test(flavor = "multi_thread")]
async fn daily_tokens_are_set_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("undaunted.db"), &Options::default(), &SCHEMA).unwrap();
    store
        .write(|conn| {
            diesel::sql_query(
                r#"INSERT INTO inventories (characterId, instancedItems, stackedItems) VALUES
                ('a', '[]', '[{"catalogId":"ORE","quantity":3},{"catalogId":"TOKEN_DAILY_PATROL_BONUS","quantity":2}]'),
                ('b', '[]', '[{"catalogId":"TOKEN_DAILY_PATROL_BONUS","quantity":12},{"catalogId":"TOKEN_BOUNTY_DRAFT","quantity":6}]'),
                ('c', '[]', 'not json')"#,
            )
            .execute(conn)
            .map_err(store::Error::from)
        })
        .await
        .unwrap();
    let settings = Settings {
        patrol_bonus_daily: 12,
        ..Settings::default()
    };
    let first = store
        .write(move |conn| set_daily_tokens_once(conn, &settings).map_err(store::Error::from))
        .await
        .unwrap();
    assert_eq!(first, Some(1));
    let second = store
        .write(move |conn| set_daily_tokens_once(conn, &settings).map_err(store::Error::from))
        .await
        .unwrap();
    assert_eq!(second, None);

    let stacks: Vec<String> = store
        .read(|conn| {
            metagame::schema::inventories::table
                .select(metagame::schema::inventories::stackedItems)
                .order(metagame::schema::inventories::characterId)
                .load(conn)
                .map_err(store::Error::from)
        })
        .await
        .unwrap();
    assert_eq!(
        stacks,
        [
            r#"[{"catalogId":"ORE","quantity":3},{"catalogId":"TOKEN_DAILY_PATROL_BONUS","quantity":12},{"catalogId":"TOKEN_BOUNTY_DRAFT","quantity":6}]"#,
            r#"[{"catalogId":"TOKEN_DAILY_PATROL_BONUS","quantity":12},{"catalogId":"TOKEN_BOUNTY_DRAFT","quantity":6}]"#,
            "not json",
        ]
    );
}
