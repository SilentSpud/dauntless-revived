#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! `metagame invite` (`new-invite.mjs`): the invite code lands in the database and the printed
//! invite carries it, with the gateway certificate's fingerprint in public mode.

use base64::Engine;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use metagame::db::SCHEMA;
use sha2::{Digest, Sha256};
use store::{Options, Store};

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn query(invite: &str) -> std::collections::HashMap<String, String> {
    let (_, query) = invite.split_once('?').unwrap();
    query
        .split('&')
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap();
            (k.to_owned(), percent_decode(v))
        })
        .collect()
}

fn percent_decode(text: &str) -> String {
    service_kit::js::decode_uri_component(text).unwrap()
}

#[test]
fn invites_for_public_and_private_servers() {
    for mode in ["public", "private"] {
        let dir = tempfile::tempdir().unwrap();
        let (config, data) = (dir.path().join("config"), dir.path().join("data"));
        std::fs::create_dir_all(data.join("tls")).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        let db = data.join("undaunted.db");
        drop(Store::open(&db, &Options::default(), &SCHEMA).unwrap());
        std::fs::write(
            config.join("server.conf"),
            format!(
                "DR_ROOT={}\nDR_DATA={}\nDR_MODE={mode}\nDR_MY_IP=203.0.113.10\nDR_GATEWAY_EXTERNAL_PORT=8443\n",
                dir.path().display(),
                data.display()
            ),
        )
        .unwrap();
        std::fs::write(
            config.join("metagame.env"),
            format!("DB_FILENAME=\"{}\"\n", db.display()),
        )
        .unwrap();
        let der = b"a test certificate";
        let pem = format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            base64::engine::general_purpose::STANDARD.encode(der)
        );
        std::fs::write(data.join("tls").join("gateway-cert.pem"), pem).unwrap();

        let args: Vec<String> = ["--config", config.to_str().unwrap(), "--name", "Test Server"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let invite = metagame::kitinvite::run(&args).unwrap();
        let q = query(&invite);
        assert_eq!(q["host"], "203.0.113.10");
        assert_eq!(q["name"], "Test Server");
        assert_eq!(q["code"].len(), 20);
        if mode == "public" {
            assert_eq!(
                (q["v"].as_str(), q["mode"].as_str(), q["port"].as_str()),
                ("2", "public", "8443")
            );
            let fp: String = Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(q["fp"], fp);
        } else {
            assert_eq!((q["v"].as_str(), q["port"].as_str()), ("1", "61000"));
        }
        let mut conn = SqliteConnection::establish(db.to_str().unwrap()).unwrap();
        let found =
            diesel::sql_query("SELECT count(*) AS n FROM invitecodes WHERE invitecode = ? AND usesRemaining = 1")
                .bind::<Text, _>(&q["code"])
                .get_result::<Count>(&mut conn)
                .unwrap();
        assert_eq!(found.n, 1, "{mode}");
    }
}
