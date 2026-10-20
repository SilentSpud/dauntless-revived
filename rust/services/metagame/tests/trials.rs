#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The parts of Trials that depend on the week, checked against Node's output at fixed weeks and
//! times (`fixtures/trials-rotation.json`, written by `fixtures/make-trials-rotation.cjs`): the
//! behemoth and rows of each week, the week windows, the tuning schedule, and the reward ranks.

use metagame::store::Settings;
use metagame::trials;
use serde_json::{Value, json};
use service_kit::env::Env;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/trials-rotation.json")).unwrap()
}

fn at(text: &str) -> i64 {
    metagame::js::date_parse(text).unwrap()
}

#[test]
fn weekly_rotation_matches_node() {
    for want in fixture()["weeks"].as_array().unwrap() {
        let week = want["week"].as_i64().unwrap();
        let got = json!({
            "week": week,
            "behemoth": trials::behemoth_for_week(week).unwrap(),
            "hard": trials::trial_id_for_week(0, week).unwrap(),
            "elite": trials::trial_id_for_week(1, week).unwrap(),
        });
        assert_eq!(&got, want, "week {week}");
    }
}

#[test]
fn windows_and_schedule_match_node() {
    let start = metagame::js::date_parse(trials::ROTATION_START);
    let settings = Settings::from_env(&Env::from_pairs([("TRIALS_SCHEDULE", "on")]));
    for want in fixture()["schedules"].as_array().unwrap() {
        let now = at(want["at"].as_str().unwrap());
        let week = trials::week_at(start, now).unwrap();
        assert_eq!(json!(week), want["week"], "{}", want["at"]);
        let (trial_start, trial_end, time_to_refresh) = trials::window_for_week(start.unwrap(), week, now);
        assert_eq!(
            json!({"trial_start": trial_start, "trial_end": trial_end, "time_to_refresh": time_to_refresh}),
            want["window"],
            "{}",
            want["at"]
        );
        assert_eq!(
            trials::seasonal_event_schedule(&settings, now).unwrap(),
            want["schedule"],
            "{}",
            want["at"]
        );
    }
}

#[test]
fn schedule_is_empty_when_off() {
    assert_eq!(
        trials::seasonal_event_schedule(&Settings::default(), at("2026-10-20T09:00:00.000Z")).unwrap(),
        json!({"code": null, "message": "OK", "payload": {"ScheduledItems": []}})
    );
}

#[test]
fn reward_ranks_match_node() {
    for want in fixture()["ranks"].as_array().unwrap() {
        assert_eq!(
            json!(trials::reward_rank(want["time"].as_i64().unwrap())),
            want["rank"],
            "{}",
            want["time"]
        );
    }
}
