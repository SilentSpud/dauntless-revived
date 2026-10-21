//! `gameserveronly.test.ts`: the guard for the game-server-only writes, and the note for a game
//! server's request that carries another account's token (logged, never refused).

use metagame::auth::MetagameAuth;
use metagame::crypto::TokenClaims;
use metagame::progression::{note_relayed_mismatch, refuse_unless_game_server};

use crate::common::warnings;

fn claims(user: &str) -> TokenClaims {
    TokenClaims {
        user_id: user.to_owned(),
        epoch: None,
    }
}

#[test]
fn refuses_a_player_s_request_with_403_and_lets_a_game_server_s_through() {
    let player = MetagameAuth::Player(claims("UID-p"));
    let ((cooldown, loadout), logged) = warnings(|| {
        (
            refuse_unless_game_server(&player, Some("UID-p"), "cooldown batch", "UID-p"),
            refuse_unless_game_server(&player, Some("UID-p"), "loadout slot unlock", "characterId c-1"),
        )
    });
    assert_eq!(cooldown.unwrap().status(), 403);
    assert_eq!(loadout.unwrap().status(), 403);
    assert_eq!(
        logged,
        [
            "Refusing cooldown batch for UID-p from a player client",
            "Refusing loadout slot unlock for characterId c-1 from a player client",
        ]
    );
    let (server, logged) = warnings(|| {
        refuse_unless_game_server(
            &MetagameAuth::GameServer(None),
            Some("UID-p"),
            "cooldown batch",
            "UID-p",
        )
    });
    assert!(server.is_none());
    assert!(logged.is_empty());
}

#[test]
fn logs_a_game_server_s_request_that_carries_another_account_s_token_once_a_minute_and_never_refuses_it() {
    let relayed = MetagameAuth::GameServer(Some(claims("UID-token-owner-1")));
    let (refused, logged) =
        warnings(|| refuse_unless_game_server(&relayed, Some("UID-named-1"), "bounty update", "UID-named-1"));
    assert!(refused.is_none(), "not refused");
    assert_eq!(
        logged,
        [
            "Game server bounty update for UID-named-1 carries the token of UID-token-owner-1: accepted for UID-named-1, the account the request names"
        ]
    );
    let (again, logged) = warnings(|| note_relayed_mismatch(&relayed, Some("UID-named-1"), "bounty update"));
    assert!(again);
    assert!(
        logged.is_empty(),
        "the same mismatch again within a minute is not logged again"
    );
    let (other, logged) = warnings(|| note_relayed_mismatch(&relayed, Some("UID-named-1"), "cooldown batch"));
    assert!(other);
    assert_eq!(logged.len(), 1);
}

#[test]
fn is_quiet_for_the_token_s_own_account_no_token_and_a_player_s_request() {
    let (results, logged) = warnings(|| {
        [
            note_relayed_mismatch(
                &MetagameAuth::GameServer(Some(claims("UID-named"))),
                Some("UID-named"),
                "bounty update",
            ),
            note_relayed_mismatch(&MetagameAuth::GameServer(None), Some("UID-named"), "bounty update"),
            note_relayed_mismatch(
                &MetagameAuth::GameServer(Some(claims(""))),
                Some("UID-named"),
                "bounty update",
            ),
            note_relayed_mismatch(
                &MetagameAuth::Player(claims("UID-token-owner")),
                Some("UID-named"),
                "bounty update",
            ),
        ]
    });
    assert_eq!(results, [false; 4]);
    assert!(logged.is_empty());
}
