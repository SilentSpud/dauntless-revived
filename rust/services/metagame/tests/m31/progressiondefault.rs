//! `progressiondefault.test.ts`: what a server with no progression settings answers, over HTTP. Real
//! progression is the default: a new account starts at 0 and keeps what it earns, owns the Elite Hunt
//! Pass, and its claims are stored.

use serde_json::{Value, json};

use crate::common::{TestApp, req};

#[tokio::test(flavor = "multi_thread")]
async fn a_new_account_reads_its_own_tracks_at_0_as_a_plain_list_not_the_stub_s_99_999_999() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let tracks = app.get(&format!("/progression/{user}"), req().bearer(&user)).await;
    assert_eq!((tracks.status, tracks.json["code"].clone()), (200, json!(200)));
    let list = tracks.json["payload"].as_array().expect("real mode sends an array");
    assert_eq!(list.len(), 10);
    for track in list {
        assert_eq!(track["phx_account_id"], user);
        assert_eq!(
            json!([
                track["progress"],
                track["confirmed_fremium_rank"],
                track["confirmed_premium_rank"]
            ]),
            json!([0, 0, 0]),
            "{}",
            track["progression_id"]
        );
    }
    let objectives = app
        .get(&format!("/progression/objectives/{user}"), req().bearer(&user))
        .await;
    assert_eq!(
        (objectives.status, objectives.json["payload"].clone()),
        (200, json!([]))
    );
    let pass = app.get(&format!("/huntpass/{user}"), req().bearer(&user)).await;
    assert_eq!((pass.status, pass.json["payload"].clone()), (200, json!("season09b")));
}

#[tokio::test(flavor = "multi_thread")]
async fn every_account_owns_the_elite_hunt_pass_season09b_premium_so_the_elite_track_is_not_locked() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let reply = app.get("/entitlementsv2", req().bearer(&user)).await;
    assert_eq!(reply.status, 200);
    assert!(
        reply.json["entitlements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "season09b_premium")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn stores_xp_from_the_game_server_and_gives_it_back_after_a_fresh_read() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let body = json!({"progress_tracks": [{"progression_id": "MasteryTrack_PlayerLevel", "progress": 8}],
        "objectives": [{"objective_id": "OBJ_DEFAULT", "value": 2, "completed_count": 0}]});
    let grant = app
        .post(&format!("/progression/{user}"), req().gs().bearer(&user).json(body))
        .await;
    assert_eq!(grant.status, 200, "the stub refused every grant with 400");
    let echoed: Vec<Value> = grant.json["payload"]["objectives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| json!([o["objective_id"], o["progress"]]))
        .collect();
    assert_eq!(
        echoed,
        [json!(["OBJ_DEFAULT", 2])],
        "objectives are echoed (the endless mastery pop fix)"
    );
    let tracks = app.get(&format!("/progression/{user}"), req().bearer(&user)).await;
    let level = tracks.json["payload"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["progression_id"] == "MasteryTrack_PlayerLevel")
        .unwrap()
        .clone();
    assert_eq!(level["progress"], 8);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hunt_pass_claim_on_the_free_and_the_elite_track_is_stored_the_confirm_routes_exist() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        app.post(&format!("/progression/{user}/season09b/100"), req().gs().bearer(&user))
            .await
            .status,
        200
    );
    let free = app
        .post(
            &format!("/progression/{user}/season09b/1/confirm/public"),
            req().gs().bearer(&user),
        )
        .await;
    let elite = app
        .post(
            &format!("/progression/{user}/season09b/1/confirm/premium"),
            req().gs().bearer(&user),
        )
        .await;
    assert_eq!((free.status, elite.status), (200, 200));
    let track = app
        .get(&format!("/progression/{user}/season09b"), req().bearer(&user))
        .await
        .json["payload"]
        .clone();
    assert_eq!(
        json!([
            track["progress"],
            track["confirmed_fremium_rank"],
            track["confirmed_premium_rank"]
        ]),
        json!([100, 1, 1])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_s_own_client_still_cannot_grant_itself_xp() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let body = json!({"progress_tracks": [{"progression_id": "MasteryTrack_PlayerLevel", "progress": 1000}], "objectives": []});
    assert_eq!(
        app.post(&format!("/progression/{user}"), req().bearer(&user).json(body))
            .await
            .status,
        403
    );
}
