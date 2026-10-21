#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! Starting, restarting and stopping game servers (`test/persistentworlds.test.ts`,
//! `launch-latency.test.ts`, `cityinstances.test.ts`, `hunt-contract.test.ts`). Every spawn is a
//! stand-in, and the one test that uses the real spawn points at a file that does not exist.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use deploy::capacity::{DeployError, Reason};
use deploy::matchmaker::Matchmaker;
use deploy::process::{BoxFuture, GameProcess, Platform, RealPlatform, SpawnOptions};
use deploy::remote::Region;
use serde_json::json;

fn city() -> serde_json::Value {
    json!({ "GameMode": "CITY", "GameArgs": "", "HuntId": "ShatteredIsles_ReturnToRamsgate" })
}

fn ramsgate_pid(w: &World) -> u32 {
    w.deploy.state_for_tests().ramsgate.unwrap().process_id
}

fn free_ports(w: &World) -> Vec<u16> {
    let mut ports = w.deploy.state_for_tests().free_ports;
    ports.sort_unstable();
    ports
}

#[tokio::test(flavor = "multi_thread")]
async fn console_visibility_is_kept_unless_deploy_says_otherwise() {
    for diagnostic in [None, Some("0"), Some("1"), Some("invalid")] {
        let w = world(Mode::Running, &[]);
        w.deploy.settings.set("DR_SHOW_CONSOLE", diagnostic);
        w.deploy.startup().await.unwrap();
        w.deploy.dojo_connection().await.unwrap();
        w.deploy.start_with_args(TUTORIAL_ARGS).await.unwrap();
        let spawned = w.platform.spawned();
        assert_eq!(spawned.len(), 3);
        for (index, child) in spawned.iter().enumerate() {
            let expected = match diagnostic {
                Some("0") => true,
                Some("1") => false,
                _ => index == 2,
            };
            assert_eq!(child.hide, expected, "{diagnostic:?} {index}");
            assert_eq!(child.console.as_deref(), diagnostic);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn hard_primary_capacity_admits_one_concurrent_last_slot() {
    let w = world(Mode::Running, &[]);
    w.deploy.startup().await.unwrap();
    w.deploy.dojo_connection().await.unwrap();
    w.deploy.settings.set("MAX_LOCAL_HUNTS", Some("1"));
    let results = futures_util::future::join_all((0..8).map(|_| w.deploy.start_with_args(TUTORIAL_ARGS))).await;
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(w.platform.count(), 3);
    assert_eq!(w.deploy.gameservers().iter().filter(|s| !s.persistent()).count(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_limits_pending_launches_and_running_hunts() {
    let w = world(
        Mode::Running,
        &[("HUNT_WORKER", "1"), ("MAX_HUNTS", "3"), ("MAX_PENDING_HUNTS", "2")],
    );
    w.deploy.startup().await.unwrap();
    let results = futures_util::future::join_all((0..8).map(|_| w.deploy.start_with_args(TUTORIAL_ARGS))).await;
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        2,
        "only two launches may be pending at once"
    );
    for result in results.iter().filter_map(|r| r.as_ref().err()) {
        assert_eq!(result, &DeployError::Capacity(Reason::Hunts));
    }
    assert_eq!(w.platform.count(), 2);
    // Once the burst has spawned, the worker still has its third running slot
    w.deploy.start_with_args(TUTORIAL_ARGS).await.unwrap();
    assert_eq!(w.platform.count(), 3);
    assert_eq!(
        w.deploy.start_with_args(TUTORIAL_ARGS).await.unwrap_err(),
        DeployError::Capacity(Reason::Hunts)
    );
    assert_eq!(w.platform.count(), 3, "a full worker must not spawn a fourth hunt");
}

#[tokio::test(flavor = "multi_thread")]
async fn low_memory_blocks_new_worlds_before_spawn() {
    let w = world(Mode::Running, &[]);
    w.deploy.startup().await.unwrap();
    w.deploy.settings.set("GAMESERVER_MEMORY_GUARD", Some("1"));
    w.deploy.settings.set("GAMESERVER_MIN_FREE_MB", Some("999999999"));
    let ramsgate = w.deploy.ramsgate_connection(1).await.unwrap();
    assert_eq!((ramsgate.host.as_str(), ramsgate.port), ("127.0.0.1", 8777));
    assert_eq!(
        w.deploy.dojo_connection().await.unwrap_err(),
        DeployError::Capacity(Reason::Memory)
    );
    assert_eq!(
        w.deploy.start_with_args(TUTORIAL_ARGS).await.unwrap_err(),
        DeployError::Capacity(Reason::Memory)
    );
    assert_eq!(w.platform.count(), 1);
    assert_eq!(free_ports(&w), HUNT_POOL);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_to_spawn_over_a_stale_listener_on_ramsgates_port() {
    let w = world(Mode::Running, &[]);
    *w.platform.port_bound.lock().unwrap() = Arc::new(|port| port == 8777);
    let error = w.deploy.startup().await.unwrap_err();
    assert!(error.to_string().contains("port 8777 is already in use"), "{error}");
    assert_eq!(w.platform.count(), 0);
    assert!(w.deploy.gameservers().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn ramsgate_starts_once_and_is_handed_out_while_it_runs() {
    let w = world(Mode::Running, &[]);
    let matchmaker = Matchmaker::new(w.deploy.clone());
    w.deploy.startup().await.unwrap();
    let spawned = w.platform.spawned();
    assert_eq!(spawned.len(), 1);
    assert_eq!(
        (spawned[0].args[1].as_str(), spawned[0].args[2].as_str()),
        ("8777", "/Game/Maps/ramsgate/ramsgate_01_persistent")
    );
    assert_eq!(free_ports(&w), HUNT_POOL);
    let id = w.deploy.state_for_tests().ramsgate.unwrap().id;
    let details = w.deploy.ramsgate_connection(1).await.unwrap();
    assert_eq!((details.port, details.session_id.as_deref()), (8777, Some(id.as_str())));
    let matched = matchmaker.handle(&city(), Region::Main).await.unwrap();
    assert_eq!((matched.port, matched.session_id), (8777, Some(id)));
    assert_eq!(w.platform.count(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watchdog_restart_stores_the_new_record() {
    let w = world(Mode::Running, &[]);
    w.deploy.startup().await.unwrap();
    let first = w.deploy.state_for_tests().ramsgate.unwrap();
    w.platform.kill(first.process_id);
    w.deploy.cleanup_server(&first).await.unwrap();
    let second = w.deploy.state_for_tests().ramsgate.unwrap();
    assert_ne!(second.process_id, first.process_id);
    assert_eq!(second.port, 8777);
    let pids: Vec<u32> = w.deploy.gameservers().iter().map(|s| s.process_id).collect();
    assert_eq!(pids, [second.process_id], "the dead record is gone");
    w.deploy.ramsgate_connection(1).await.unwrap();
    assert_eq!(w.platform.count(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn ramsgate_starts_again_when_a_player_travels_there_after_it_ended() {
    let _serial = serial().await;
    let w = world(Mode::Running, &[]);
    let matchmaker = Matchmaker::new(w.deploy.clone());
    w.deploy.startup().await.unwrap();
    w.platform.kill(ramsgate_pid(&w));
    let before = logged().len();
    assert_eq!(matchmaker.handle(&city(), Region::Main).await.unwrap().port, 8777);
    assert_eq!(w.platform.count(), 2);
    assert!(
        logged()[before..].contains(
            &"warn: Ramsgate is not running any more: starting it again before sending anyone there".to_owned()
        )
    );
    let servers = w.deploy.gameservers();
    assert_eq!(servers.len(), 1);
    assert_eq!(Some(servers[0].process_id), w.platform.spawned()[1].pid);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watchdog_restart_and_a_city_request_together_start_one_process() {
    let w = world(Mode::Running, &[]);
    let matchmaker = Matchmaker::new(w.deploy.clone());
    w.deploy.startup().await.unwrap();
    w.platform.kill(ramsgate_pid(&w));
    w.deploy.run_watchdog().await;
    let empty = json!({ "GameMode": "CITY", "GameArgs": "", "HuntId": "" });
    let (a, b, c) = tokio::join!(
        w.deploy.ramsgate_connection(1),
        w.deploy.ramsgate_connection(1),
        matchmaker.handle(&empty, Region::Main)
    );
    assert!(
        [a.unwrap().port, b.unwrap().port, c.unwrap().port]
            .iter()
            .all(|p| *p == 8777)
    );
    wait_for("the new Ramsgate", || {
        Some(ramsgate_pid(&w)) == w.platform.spawned().get(1).and_then(|s| s.pid)
    })
    .await;
    assert_eq!(w.platform.count(), 2);

    // The other way round: the request first, then the watchdog
    w.platform.kill(ramsgate_pid(&w));
    let deploy = w.deploy.clone();
    let request = tokio::spawn(async move { deploy.ramsgate_connection(1).await });
    w.deploy.run_watchdog().await;
    request.await.unwrap().unwrap();
    assert_eq!(w.platform.count(), 3);
    assert_eq!(w.deploy.gameservers().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watchdog_round_holding_the_dead_record_does_not_start_a_second_ramsgate() {
    let w = world(Mode::Running, &[]);
    w.deploy.startup().await.unwrap();
    let dead = w.deploy.state_for_tests().ramsgate.unwrap();
    w.platform.kill(dead.process_id);
    w.deploy.ramsgate_connection(1).await.unwrap();
    w.deploy.cleanup_server(&dead).await.unwrap();
    assert_eq!(w.platform.count(), 2);
    assert_eq!(w.deploy.gameservers().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_liveness_ramsgate_is_left_to_the_watchdog() {
    let w = world(Mode::Running, &[("PERSISTENT_WORLD_LIVENESS", "0")]);
    w.deploy.startup().await.unwrap();
    let dead = w.deploy.state_for_tests().ramsgate.unwrap();
    w.platform.kill(dead.process_id);
    let details = w.deploy.ramsgate_connection(1).await.unwrap();
    assert_eq!(details.session_id.as_deref(), Some(dead.id.as_str()));
    assert_eq!(w.platform.count(), 1, "no liveness restart");
    w.deploy.run_watchdog().await;
    wait_for("the watchdog's restart", || {
        w.deploy.state_for_tests().ramsgate.unwrap().id != dead.id
    })
    .await;
    assert_eq!(w.platform.count(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_during_boot_waits_for_the_boots_start() {
    let w = world(Mode::Running, &[]);
    let (boot, request) = tokio::join!(w.deploy.startup(), w.deploy.ramsgate_connection(1));
    boot.unwrap();
    request.unwrap();
    assert_eq!(w.platform.count(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_dojo_stays_stopped_after_cleanup_and_starts_once_on_demand() {
    let w = world(Mode::Running, &[]);
    w.deploy.startup().await.unwrap();
    w.deploy.dojo_connection().await.unwrap();
    let dead = w.deploy.state_for_tests().dojo.unwrap();
    w.platform.kill(dead.process_id);
    w.deploy.cleanup_server(&dead).await.unwrap();
    w.deploy.run_watchdog().await;
    assert_eq!(w.platform.count(), 2);
    assert!(w.deploy.state_for_tests().dojo.is_none());
    assert!(!w.deploy.gameservers().iter().any(|s| s.id == dead.id));
    let (a, b) = tokio::join!(w.deploy.dojo_connection(), w.deploy.dojo_connection());
    a.unwrap();
    b.unwrap();
    assert_eq!(w.platform.count(), 3);
    assert_ne!(w.deploy.state_for_tests().dojo.unwrap().process_id, dead.process_id);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_dojo_starts_on_first_use_once_and_again_after_it_ended() {
    let _serial = serial().await;
    let w = world(Mode::Running, &[]);
    let matchmaker = Matchmaker::new(w.deploy.clone());
    w.deploy.startup().await.unwrap();
    let before = logged().len();
    let shared = json!({ "GameMode": "SHARED", "GameArgs": "", "HuntId": "ShatteredIsles_TrainingDojo" });
    let (a, b, c) = tokio::join!(
        w.deploy.dojo_connection(),
        w.deploy.dojo_connection(),
        matchmaker.handle(&shared, Region::Main)
    );
    assert!(
        [a.unwrap().port, b.unwrap().port, c.unwrap().port]
            .iter()
            .all(|p| *p == 8776)
    );
    assert_eq!(w.platform.count(), 2);
    assert!(logged()[before..].contains(&"info: Starting the Training Dojo on demand".to_owned()));
    w.deploy.dojo_connection().await.unwrap();
    assert_eq!(w.platform.count(), 2);
    w.platform.kill(w.deploy.state_for_tests().dojo.unwrap().process_id);
    assert_eq!(w.deploy.dojo_connection().await.unwrap().port, 8776);
    assert_eq!(w.platform.count(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn with_enable_dojo_both_worlds_start_at_boot() {
    let w = world(Mode::Running, &[("ENABLE_DOJO", "1")]);
    let (boot, request) = tokio::join!(w.deploy.startup(), w.deploy.dojo_connection());
    boot.unwrap();
    request.unwrap();
    let mut ports: Vec<String> = w.platform.spawned().iter().map(|s| s.args[1].clone()).collect();
    ports.sort();
    assert_eq!(ports, ["8776", "8777"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_watchdog_returns_a_finished_hunts_port() {
    let w = world(Mode::Running, &[]);
    w.deploy.startup().await.unwrap();
    let hunt = w.deploy.start_with_args(TUTORIAL_ARGS).await.unwrap();
    assert!(HUNT_POOL.contains(&hunt.port));
    assert!(!free_ports(&w).contains(&hunt.port));
    let pid = w
        .deploy
        .gameservers()
        .iter()
        .find(|s| s.port == hunt.port)
        .unwrap()
        .process_id;
    w.platform.kill(pid);
    w.deploy.run_watchdog().await;
    assert_eq!(free_ports(&w), HUNT_POOL);
    assert!(!w.deploy.gameservers().iter().any(|s| s.port == hunt.port));
    assert_eq!(w.platform.count(), 2, "Ramsgate was not touched");
}

/// The real spawn with the stand-ins' idea of what is alive.
struct RealSpawn(Arc<FakePlatform>);

impl Platform for RealSpawn {
    fn spawn(&self, command: &str, args: &[String], options: &SpawnOptions) -> Result<Arc<dyn GameProcess>, String> {
        RealPlatform.spawn(command, args, options)
    }

    fn is_alive(&self, pid: u32) -> bool {
        self.0.is_alive(pid)
    }

    fn is_port_bound(&self, port: u16) -> BoxFuture<Result<bool, String>> {
        self.0.is_port_bound(port)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_binary_is_logged_and_gives_the_port_back() {
    let _serial = serial().await;
    let w = world(Mode::Running, &[]);
    w.deploy.startup().await.unwrap();
    w.deploy.use_platform(Arc::new(RealSpawn(w.platform.clone())));
    let before = logged().len();
    let error = w.deploy.start_with_args(TUTORIAL_ARGS).await.unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("Could not start a game server on port 877"),
        "{error}"
    );
    let binary = w.dir.path().join("missing").join("Dauntless-Win64-Shipping.exe");
    let lines = logged()[before..].to_vec();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("error: Game server on port") && l.contains(binary.to_str().unwrap())),
        "{lines:?}"
    );
    assert_eq!(free_ports(&w), HUNT_POOL);
    assert_eq!(w.deploy.gameservers().len(), 1, "only Ramsgate");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_ramsgate_restart_is_an_error_for_the_request_and_a_line_for_the_watchdog() {
    let _serial = serial().await;
    let w = world(Mode::Running, &[]);
    w.deploy.startup().await.unwrap();
    let dead = w.deploy.state_for_tests().ramsgate.unwrap();
    w.platform.kill(dead.process_id);
    w.platform.set_mode(Mode::Failing);
    let error = w.deploy.ramsgate_connection(1).await.unwrap_err();
    assert_eq!(error.to_string(), "Could not start a game server on port 8777");
    let before = logged().len();
    w.deploy.push_for_tests(dead);
    w.deploy.run_watchdog().await;
    wait_for("the watchdog's line", || {
        logged()[before..]
            .iter()
            .any(|l| l.starts_with("error: Could not restart the game server on port 8777"))
    })
    .await;
    w.platform.set_mode(Mode::Running);
    assert_eq!(w.deploy.ramsgate_connection(1).await.unwrap().port, 8777);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_startup_is_one_fatal_line_and_a_failure_exit() {
    let _serial = serial().await;
    let w = world(Mode::Failing, &[]);
    let before = logged().len();
    w.deploy.startup_and_report_failure().await;
    assert!(
        logged()[before..].contains(
            &"fatal: Starting the game servers failed: Could not start a game server on port 8777".to_owned()
        ),
        "{:?}",
        &logged()[before..]
    );
    assert!(w.deploy.startup_failed());
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_stops_its_game_servers_without_restarting_ramsgate() {
    let w = world(Mode::Killable, &[("RAMSGATE_POOL", "1")]);
    w.deploy.startup().await.unwrap();
    w.deploy.ramsgate_connection(1).await.unwrap();
    w.deploy.start_with_args(TUTORIAL_ARGS).await.unwrap();
    let pids: Vec<u32> = w.deploy.gameservers().iter().map(|s| s.process_id).collect();
    let count = w.platform.count();
    assert_eq!(pids.len(), 2);
    assert!(pids.iter().all(|pid| w.platform.is_alive(*pid)));
    w.deploy.shutdown().await;
    assert!(pids.iter().all(|pid| !w.platform.is_alive(*pid)));
    assert!(w.deploy.gameservers().is_empty());
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(w.platform.count(), count, "shutdown must not restart Ramsgate");
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_interrupts_a_start_in_flight_and_prevents_new_ones() {
    let w = world(Mode::Killable, &[("GAMESERVER_STARTUP_GRACE_MS", "5000")]);
    let deploy = w.deploy.clone();
    let boot = tokio::spawn(async move { deploy.startup_and_report_failure().await });
    wait_for("the spawn", || w.platform.count() == 1).await;
    let pid = w.platform.spawned()[0].pid.unwrap();
    assert!(w.platform.is_alive(pid), "Ramsgate is still starting");
    w.deploy.shutdown().await;
    boot.await.unwrap();
    assert!(!w.platform.is_alive(pid));
    assert!(w.deploy.gameservers().is_empty());
    assert!(
        !w.deploy.startup_failed(),
        "an intentional shutdown is not a startup failure"
    );
    let error = w.deploy.start_with_args(TUTORIAL_ARGS).await.unwrap_err();
    assert_eq!(error.to_string(), "Deploy service is shutting down");
    assert_eq!(w.platform.count(), 1, "no game server may spawn after shutdown begins");
}

// ---- launch-latency.test.ts ----

const ARGS: &str = "/Game/Test?MaxPlayers=1?MonsterClass=test";

#[tokio::test(flavor = "multi_thread")]
async fn cold_starts_overlap_while_spacing_and_readiness_hold() {
    // Readiness probes the real UDP ports
    let _serial = serial().await;
    let w = world(Mode::Running, &[("SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP", "0.3")]);
    *w.platform.all_alive.lock().unwrap() = true;
    w.deploy.startup().await.unwrap();
    w.deploy.settings.set("GAMESERVER_STARTUP_GRACE_MS", Some("90000"));
    let (first, second) = (w.deploy.start_with_args(ARGS), w.deploy.start_with_args(ARGS));
    let platform = w.platform.clone();
    let markers = async move {
        tokio::time::sleep(Duration::from_millis(850)).await;
        let spawned: Vec<Spawned> = platform.spawned().into_iter().skip(1).collect();
        assert_eq!(spawned.len(), 2, "one cold start blocked the entire launch queue");
        for marker in spawned {
            std::fs::write(
                marker.ready_file.unwrap(),
                format!("{}:{}", marker.pid.unwrap(), marker.args[1]),
            )
            .unwrap();
        }
    };
    let (first, second, ()) = tokio::join!(first, second, markers);
    first.unwrap();
    second.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_spawns_add_no_cooldown_and_successful_ones_are_spaced() {
    let w = world(Mode::Throwing, &[("SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP", "0.3")]);
    let started = tokio::time::Instant::now();
    for _ in 0..3 {
        let error = w.deploy.startup().await.unwrap_err();
        assert!(error.to_string().contains("Could not start"));
        w.deploy.reset_for_tests();
    }
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "failed spawns accumulated cooldowns"
    );
    w.platform.set_mode(Mode::Running);
    *w.platform.all_alive.lock().unwrap() = true;
    w.deploy.startup().await.unwrap();
    let launched = tokio::time::Instant::now();
    w.deploy.start_with_args(ARGS).await.unwrap();
    assert!(
        launched.elapsed() >= Duration::from_millis(250),
        "successful launches were not spaced"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hunts_exit_frees_its_port_at_once_and_the_watchdog_cannot_duplicate_it() {
    let w = world(Mode::Running, &[]);
    *w.platform.all_alive.lock().unwrap() = true;
    w.deploy.startup().await.unwrap();
    w.deploy.start_with_args(ARGS).await.unwrap();
    let hunt = w.deploy.gameservers().into_iter().find(|s| !s.is_ramsgate).unwrap();
    w.platform.exit(hunt.process_id, 0);
    wait_for("the exit", || !w.deploy.gameservers().iter().any(|s| s.id == hunt.id)).await;
    let count = || {
        w.deploy
            .state_for_tests()
            .free_ports
            .iter()
            .filter(|p| **p == hunt.port)
            .count()
    };
    assert_eq!(count(), 1);
    w.deploy.cleanup_server(&hunt).await.unwrap();
    assert_eq!(count(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hunt_that_exits_during_its_grace_fails_and_returns_its_port() {
    // Readiness probes the real UDP ports
    let _serial = serial().await;
    let w = world(Mode::Running, &[]);
    *w.platform.all_alive.lock().unwrap() = true;
    w.deploy.startup().await.unwrap();
    let before = w.deploy.state_for_tests().free_ports.len();
    w.deploy.settings.set("GAMESERVER_STARTUP_GRACE_MS", Some("500"));
    let platform = Arc::downgrade(&w.platform);
    *w.platform.on_spawn.lock().unwrap() = Some(Arc::new(move |spawned: &Spawned| {
        let (platform, pid) = (platform.clone(), spawned.pid.unwrap());
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if let Some(platform) = platform.upgrade() {
                platform.exit(pid, 1);
            }
        });
    }));
    let error = w.deploy.start_with_args(ARGS).await.unwrap_err();
    assert!(error.to_string().contains("exited before listening"), "{error}");
    wait_for("the port", || w.deploy.state_for_tests().free_ports.len() == before).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hunt_waits_for_its_udp_listener_when_no_marker_is_written() {
    // Readiness probes the real UDP ports
    let _serial = serial().await;
    let w = world(Mode::Running, &[]);
    *w.platform.all_alive.lock().unwrap() = true;
    w.deploy.startup().await.unwrap();
    w.deploy.settings.set("GAMESERVER_STARTUP_GRACE_MS", Some("3000"));
    let listener: Arc<std::sync::Mutex<Option<std::net::UdpSocket>>> = Arc::default();
    let hold = listener.clone();
    *w.platform.on_spawn.lock().unwrap() = Some(Arc::new(move |spawned: &Spawned| {
        let (hold, port) = (hold.clone(), spawned.args[1].parse::<u16>().unwrap());
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            *hold.lock().unwrap() = std::net::UdpSocket::bind(("0.0.0.0", port)).ok();
        });
    }));
    let started = tokio::time::Instant::now();
    let result = w.deploy.start_with_args(ARGS).await.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert!(result.port > 0);
    drop(listener);
}

// ---- cityinstances.test.ts ----

#[tokio::test(flavor = "multi_thread")]
async fn pooled_cities_start_on_demand_and_release_their_ports() {
    let w = world(Mode::Running, &[("RAMSGATE_POOL", "1")]);
    w.deploy.startup().await.unwrap();
    assert!(w.deploy.gameservers().is_empty(), "a fresh boot stays empty");
    let results = futures_util::future::join_all((0..21).map(|_| w.deploy.ramsgate_connection(1))).await;
    let results: Vec<_> = results.into_iter().map(Result::unwrap).collect();
    assert!(w.platform.spawned().iter().all(|s| {
        s.args
            .contains(&"/Game/Maps/ramsgate/ramsgate_01_persistent?MaxPlayers=20".to_owned())
    }));
    assert_eq!(w.deploy.gameservers().len(), 2);
    assert_eq!(results.iter().filter(|r| r.port == 8777).count(), 20);
    let extra = w.deploy.gameservers().into_iter().find(|s| s.port != 8777).unwrap();
    assert!(extra.is_ramsgate);
    assert_ne!(extra.port, 8776);
    w.platform.exit(extra.process_id, 0);
    w.deploy.cleanup_server(&extra).await.unwrap();
    wait_for("the exit", || w.deploy.gameservers().len() == 1).await;
    let next = w.deploy.ramsgate_connection(1).await.unwrap();
    assert_eq!(next.port, extra.port, "the released shared port is used for a new city");
    for city in w.deploy.gameservers() {
        w.platform.kill(city.process_id);
        w.deploy.cleanup_server(&city).await.unwrap();
    }
    assert!(w.deploy.gameservers().is_empty(), "no city is restarted when empty");
}

// ---- hunt-contract.test.ts ----

#[tokio::test(flavor = "multi_thread")]
async fn every_catalog_hunt_supplies_a_map_matchmaker_hunt_and_the_roster() {
    let w = world(Mode::Running, &[]);
    *w.platform.all_alive.lock().unwrap() = true;
    let matchmaker = deploy::tables::rows(deploy::tables::matchmaker_hunts());
    let mut checked = 0;
    for (hunt, row) in deploy::tables::rows(deploy::tables::player_hunts()) {
        let ids: Vec<&str> = row["MatchmakerHuntIDs"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["RowName"].as_str())
            .collect();
        if ids.is_empty() || ids.iter().any(|id| !matchmaker.contains_key(*id)) {
            continue;
        }
        w.deploy.reset_for_tests();
        w.deploy.startup().await.unwrap();
        w.deploy
            .start_with_hunt(hunt, &["UID-a".into(), "UID-b".into()])
            .await
            .unwrap();
        let args = w.platform.spawned().last().unwrap().args.clone();
        assert!(args[2].starts_with("/Game/"), "{hunt}");
        assert!(!args[4].is_empty() && args[4] != "NO_MM_HUNTID", "{hunt}");
        assert_eq!(args[5], format!("UID-a:{hunt},UID-b:{hunt}"), "{hunt}");
        checked += 1;
    }
    assert!(checked > 100, "only {checked} catalog hunts checked");
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_training_requests_share_a_world_with_its_rules() {
    let w = world(Mode::Running, &[]);
    *w.platform.all_alive.lock().unwrap() = true;
    let replies = futures_util::future::join_all((0..12).map(|_| w.deploy.dojo_connection())).await;
    let ports: std::collections::HashSet<u16> = replies.into_iter().map(|r| r.unwrap().port).collect();
    assert_eq!(ports.len(), 1);
    let args = w.platform.spawned().last().unwrap().args.clone();
    assert!(
        args[2].contains("?game=") && args[2].ends_with("BPGM_ArchonTrainingGrounds_C?MaxPlayers=12"),
        "{}",
        args[2]
    );
    assert_eq!(args[4], "CR19_MatchmakerHunt_ShatteredIsles_TrainingDojo");
    assert_eq!(args[5], "NO_EXPECTED_PLAYERS");
}
