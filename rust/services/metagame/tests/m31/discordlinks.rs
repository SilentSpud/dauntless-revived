//! `discordlinks.test.ts`: a Discord id links to one account and an account to one id, and linking
//! never touches the account's key.

use metagame::keyrotation::{discord_account, link_discord_account};

use crate::common::TestApp;

#[tokio::test(flavor = "multi_thread")]
async fn discord_linking_preserves_credentials_and_rejects_reassignment_on_both_sides() {
    let app = TestApp::start().await;
    app.exec(
        "INSERT INTO users (userId, name, notes) VALUES ('UID-one', 'One', 0), ('UID-two', 'Two', 0);
         INSERT INTO userapikeys (userId, keyHash) VALUES ('UID-one', 'unchanged-hash');",
    );
    let before = app.rows("SELECT * FROM userapikeys", &[]);
    let link = |discord: &'static str, user: &'static str| {
        let store = app.state.store.clone();
        async move {
            store
                .write(move |conn| link_discord_account(conn, Some(discord), Some(user)).map_err(store::Error::from))
                .await
                .unwrap()
        }
    };
    assert_eq!(link("12345678901234567", "UID-one").await, "linked");
    assert_eq!(link("12345678901234567", "UID-one").await, "linked");
    assert_eq!(link("12345678901234567", "UID-two").await, "discord_already_linked");
    assert_eq!(link("22345678901234567", "UID-one").await, "account_already_linked");
    assert_eq!(link("22345678901234567", "UID-missing").await, "invalid_key");
    assert_eq!(link("bad", "UID-two").await, "invalid_key");
    let account = app
        .state
        .store
        .read(|conn| discord_account(conn, "12345678901234567").map_err(store::Error::from))
        .await
        .unwrap();
    assert_eq!(account, Some(("UID-one".to_owned(), "One".to_owned())));
    assert_eq!(app.rows("SELECT * FROM userapikeys", &[]), before);
}
