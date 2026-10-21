//! Ramsgate capacity: the pool of city instances on this machine (`citypool.ts`) and the balance
//! between this machine's city and a worker's (`cityrouter.ts`).

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use crate::capacity::{DeployError, Reason};
use crate::settings::Settings;

pub const CITY_PLAYER_LIMIT: u64 = 20;

/// `RAMSGATE_CITY_CAPS="10,10,8"`: at most that many cities, filled in the order they started (the
/// oldest first), each admitting up to its own cap. Unset: cities of [`CITY_PLAYER_LIMIT`], started on
/// demand.
pub fn city_caps(settings: &Settings) -> Result<Option<Vec<u64>>, DeployError> {
    let Some(raw) = settings.get("RAMSGATE_CITY_CAPS").filter(|r| !r.trim().is_empty()) else {
        return Ok(None);
    };
    raw.split(',')
        .map(|v| {
            let n = service_kit::js::number(Some(&serde_json::Value::String(v.trim().to_owned())));
            (n.fract() == 0.0 && (1.0..=CITY_PLAYER_LIMIT as f64).contains(&n))
                .then_some(n as u64)
                .ok_or_else(|| DeployError::failed("Invalid RAMSGATE_CITY_CAPS"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

#[derive(Debug, Clone)]
pub struct City {
    pub id: String,
    pub players: Option<u64>,
    pub started_at: i64,
}

#[derive(Debug, Clone)]
struct Lease {
    size: u64,
    until: i64,
}

#[derive(Default)]
struct PoolState {
    retiring: HashSet<String>,
    last_players: HashMap<String, u64>,
    reservations: HashMap<String, Vec<Lease>>,
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type List = Arc<dyn Fn() -> Vec<City> + Send + Sync>;
type Start<T> = Arc<dyn Fn() -> BoxFuture<'static, Result<T, DeployError>> + Send + Sync>;
type Get<T> = Arc<dyn Fn(&str) -> Option<T> + Send + Sync>;
type Caps = Arc<dyn Fn() -> Result<Option<Vec<u64>>, DeployError> + Send + Sync>;
type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// Allocation runs one at a time, through a cold start, so concurrent requests share it.
pub struct CityPool<T: Clone + Send + 'static> {
    list: List,
    start: Start<T>,
    get: Get<T>,
    now: Clock,
    caps: Caps,
    id_of: Arc<dyn Fn(&T) -> String + Send + Sync>,
    queue: tokio::sync::Mutex<()>,
    state: Mutex<PoolState>,
}

impl<T: Clone + Send + 'static> CityPool<T> {
    pub fn new(
        list: impl Fn() -> Vec<City> + Send + Sync + 'static,
        start: impl Fn() -> BoxFuture<'static, Result<T, DeployError>> + Send + Sync + 'static,
        get: impl Fn(&str) -> Option<T> + Send + Sync + 'static,
        id_of: impl Fn(&T) -> String + Send + Sync + 'static,
        now: impl Fn() -> i64 + Send + Sync + 'static,
        caps: impl Fn() -> Result<Option<Vec<u64>>, DeployError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            list: Arc::new(list),
            start: Arc::new(start),
            get: Arc::new(get),
            now: Arc::new(now),
            caps: Arc::new(caps),
            id_of: Arc::new(id_of),
            queue: tokio::sync::Mutex::new(()),
            state: Mutex::default(),
        }
    }

    pub async fn allocate(&self, size: u64) -> Result<T, DeployError> {
        if !(1..=CITY_PLAYER_LIMIT).contains(&size) {
            return Err(DeployError::Capacity(Reason::Hunts));
        }
        let _turn = self.queue.lock().await;
        let mut cities = (self.list)();
        cities.sort_by_key(|c| c.started_at);
        let caps = (self.caps)()?;
        let chosen = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let gone: Vec<String> = state
                .reservations
                .keys()
                .filter(|id| !cities.iter().any(|c| &c.id == *id))
                .cloned()
                .collect();
            for id in gone {
                state.reservations.remove(&id);
                state.last_players.remove(&id);
                state.retiring.remove(&id);
            }
            let now = (self.now)();
            let mut slot = 0usize;
            let mut chosen = None;
            for city in &cities {
                if state.retiring.contains(&city.id) {
                    continue;
                }
                let mut leases: Vec<Lease> = state
                    .reservations
                    .get(&city.id)
                    .map(|l| l.iter().filter(|r| r.until > now).cloned().collect())
                    .unwrap_or_default();
                if let Some(players) = city.players {
                    let mut arrivals = players.saturating_sub(state.last_players.get(&city.id).copied().unwrap_or(0));
                    for lease in &mut leases {
                        let consumed = arrivals.min(lease.size);
                        lease.size -= consumed;
                        arrivals -= consumed;
                    }
                    // A city that just emptied is being retired by its native watchdog
                    let retiring = players == 0 && state.last_players.get(&city.id).copied().unwrap_or(0) > 0;
                    state.last_players.insert(city.id.clone(), players);
                    leases.retain(|r| r.size > 0);
                    state.reservations.insert(city.id.clone(), leases.clone());
                    if retiring {
                        state.retiring.insert(city.id.clone());
                        continue;
                    }
                } else {
                    state.reservations.insert(city.id.clone(), leases.clone());
                }
                // Missing telemetry is not proof of free seats. For a fresh launch, all admissions are
                // still reserved; otherwise leave an unknown city alone
                let occupied = city
                    .players
                    .unwrap_or(if now - city.started_at < 180_000 && !leases.is_empty() {
                        0
                    } else {
                        CITY_PLAYER_LIMIT
                    });
                let limit = caps
                    .as_ref()
                    .map_or(CITY_PLAYER_LIMIT, |caps| caps.get(slot).copied().unwrap_or(0));
                slot += 1;
                let leased: u64 = leases.iter().map(|r| r.size).sum();
                if occupied + leased + size <= limit {
                    chosen = (self.get)(&city.id);
                    break;
                }
            }
            if chosen.is_none() && caps.as_ref().is_some_and(|caps| slot >= caps.len()) {
                return Err(DeployError::Capacity(Reason::Hunts));
            }
            chosen
        };
        let chosen = match chosen {
            Some(chosen) => chosen,
            None => (self.start)().await?,
        };
        let id = (self.id_of)(&chosen);
        let until = (self.now)() + 180_000;
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reservations
            .entry(id)
            .or_default()
            .push(Lease { size, until });
        Ok(chosen)
    }

    pub fn reset(&self) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = PoolState::default();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Connection {
    pub host: String,
    pub port: u16,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none", default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Local,
    Remote,
}

#[derive(Debug, Clone)]
struct Reservation {
    id: u64,
    side: Side,
    size: u64,
    until: Option<i64>,
}

type Load = Arc<dyn Fn() -> BoxFuture<'static, Result<(f64, f64), DeployError>> + Send + Sync>;

/// Sends a whole party to whichever of this machine's city and a worker's is less occupied. Short
/// reservations cover the status cache and players still travelling.
pub struct CityRouter {
    load: Load,
    now: Clock,
    reservations: Mutex<(u64, Vec<Reservation>)>,
}

impl CityRouter {
    pub fn new(
        load: impl Fn() -> BoxFuture<'static, Result<(f64, f64), DeployError>> + Send + Sync + 'static,
        now: impl Fn() -> i64 + Send + Sync + 'static,
    ) -> Self {
        Self {
            load: Arc::new(load),
            now: Arc::new(now),
            reservations: Mutex::default(),
        }
    }

    pub async fn launch<L, R>(&self, size: u64, local: L, remote: R, limit: f64) -> Result<Connection, DeployError>
    where
        L: Future<Output = Result<Connection, DeployError>>,
        R: Future<Output = Result<Option<Connection>, DeployError>>,
    {
        if !(limit.fract() == 0.0 && (1.0..=32.0).contains(&limit)) {
            return Err(DeployError::failed("Invalid RAMSGATE_PLAYER_LIMIT"));
        }
        let size = size.max(1) as f64;
        let (mut local_count, mut remote_count) = (self.load)().await?;
        if ![local_count, remote_count].iter().all(|n| n.is_finite() && *n >= 0.0) {
            return Err(DeployError::failed("Invalid city occupancy"));
        }
        let now = (self.now)();
        {
            let mut held = self.reservations.lock().unwrap_or_else(PoisonError::into_inner);
            held.1.retain(|r| r.until.is_none_or(|until| until > now));
            for r in &held.1 {
                match r.side {
                    Side::Local => local_count += r.size as f64,
                    Side::Remote => remote_count += r.size as f64,
                }
            }
        }
        let order = if local_count <= remote_count {
            [Side::Local, Side::Remote]
        } else {
            [Side::Remote, Side::Local]
        };
        let (mut local, mut remote) = (Some(local), Some(remote));
        for side in order {
            let count = if side == Side::Local { local_count } else { remote_count };
            if count + size > limit {
                continue;
            }
            let id = {
                let mut held = self.reservations.lock().unwrap_or_else(PoisonError::into_inner);
                held.0 += 1;
                let id = held.0;
                held.1.push(Reservation {
                    id,
                    side,
                    size: size as u64,
                    until: None,
                });
                id
            };
            let result = match side {
                Side::Local => match local.take() {
                    Some(local) => local.await.map(Some),
                    None => Ok(None),
                },
                Side::Remote => match remote.take() {
                    Some(remote) => remote.await,
                    None => Ok(None),
                },
            };
            let mut held = self.reservations.lock().unwrap_or_else(PoisonError::into_inner);
            match result {
                Ok(Some(connection)) => {
                    let until = (self.now)() + 45_000;
                    if let Some(r) = held.1.iter_mut().find(|r| r.id == id) {
                        r.until = Some(until);
                    }
                    return Ok(connection);
                }
                Ok(None) | Err(DeployError::Capacity(_)) => held.1.retain(|r| r.id != id),
                Err(error) => {
                    held.1.retain(|r| r.id != id);
                    return Err(error);
                }
            }
        }
        Err(DeployError::Capacity(Reason::Hunts))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, Ordering};

    use super::*;

    type Cities = Arc<Mutex<Vec<City>>>;

    fn fixture(caps: Option<Vec<u64>>) -> (Arc<CityPool<String>>, Cities, Arc<AtomicI64>) {
        let now = Arc::new(AtomicI64::new(100_000));
        let cities: Cities = Arc::default();
        let (list, start, n) = (cities.clone(), cities.clone(), now.clone());
        let clock = now.clone();
        let pool = CityPool::new(
            move || list.lock().unwrap().clone(),
            move || {
                let (start, n) = (start.clone(), n.clone());
                Box::pin(async move {
                    let mut cities = start.lock().unwrap();
                    let id = (cities.len() + 1).to_string();
                    cities.push(City {
                        id: id.clone(),
                        players: None,
                        started_at: n.load(Ordering::SeqCst),
                    });
                    Ok(id)
                })
            },
            |id| Some(id.to_owned()),
            Clone::clone,
            move || clock.load(Ordering::SeqCst),
            move || Ok(caps.clone()),
        );
        (Arc::new(pool), cities, now)
    }

    fn set_players(cities: &Cities, index: usize, players: u64) {
        cities.lock().unwrap()[index].players = Some(players);
    }

    #[tokio::test]
    async fn twenty_one_travellers_share_a_cold_city_then_start_another() {
        let (pool, cities, _) = fixture(None);
        let all = futures_util::future::join_all((0..21).map(|_| pool.allocate(1))).await;
        let ids: Vec<String> = all.into_iter().map(Result::unwrap).collect();
        assert_eq!(cities.lock().unwrap().len(), 2);
        assert_eq!(ids.iter().filter(|id| *id == "1").count(), 20);
        assert_eq!(ids[20], "2");
    }

    #[tokio::test]
    async fn parties_reserve_together_and_arrivals_consume_reservations() {
        let (pool, cities, _) = fixture(None);
        for _ in 0..5 {
            pool.allocate(4).await.unwrap();
        }
        set_players(&cities, 0, 20);
        assert_eq!(pool.allocate(1).await.unwrap(), "2");
        set_players(&cities, 0, 16);
        assert_eq!(pool.allocate(4).await.unwrap(), "1");
        assert_eq!(pool.allocate(1).await.unwrap(), "2");
    }

    #[tokio::test]
    async fn missing_telemetry_after_the_grace_never_means_free_seats() {
        let (pool, cities, now) = fixture(None);
        pool.allocate(1).await.unwrap();
        now.fetch_add(180_001, Ordering::SeqCst);
        assert_eq!(pool.allocate(1).await.unwrap(), "2");
        assert_eq!(cities.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn an_emptied_city_is_not_reused_while_it_retires() {
        let (pool, cities, _) = fixture(None);
        pool.allocate(1).await.unwrap();
        set_players(&cities, 0, 1);
        pool.allocate(1).await.unwrap();
        set_players(&cities, 0, 0);
        assert_eq!(pool.allocate(1).await.unwrap(), "2");
    }

    #[tokio::test]
    async fn a_failed_cold_start_releases_the_allocator_and_oversized_parties_are_refused() {
        let calls = Arc::new(AtomicI64::new(0));
        let c = calls.clone();
        let pool: CityPool<String> = CityPool::new(
            Vec::new,
            move || {
                let first = c.fetch_add(1, Ordering::SeqCst) == 0;
                Box::pin(async move {
                    if first {
                        Err(DeployError::failed("failed"))
                    } else {
                        Ok("ok".to_owned())
                    }
                })
            },
            |_| Some("ok".to_owned()),
            Clone::clone,
            || 0,
            || Ok(None),
        );
        assert_eq!(pool.allocate(1).await.unwrap_err(), DeployError::failed("failed"));
        assert_eq!(pool.allocate(1).await.unwrap(), "ok");
        assert_eq!(
            pool.allocate(21).await.unwrap_err(),
            DeployError::Capacity(Reason::Hunts)
        );
    }

    #[tokio::test]
    async fn capped_cities_fill_in_start_order() {
        let (pool, cities, _) = fixture(Some(vec![10, 10, 8]));
        let mut ids = Vec::new();
        for _ in 0..28 {
            ids.push(pool.allocate(1).await.unwrap());
        }
        assert_eq!(cities.lock().unwrap().len(), 3);
        let count = |id: &str| ids.iter().filter(|i| *i == id).count();
        assert_eq!((count("1"), count("2"), count("3")), (10, 10, 8));
        assert_eq!(&ids[..12], [vec!["1"; 10], vec!["2"; 2]].concat().as_slice());
        assert!(pool.allocate(1).await.is_err());
        assert_eq!(cities.lock().unwrap().len(), 3);

        let (pool, _, _) = fixture(Some(vec![10, 10, 8]));
        for _ in 0..2 {
            pool.allocate(4).await.unwrap();
        }
        assert_eq!(pool.allocate(4).await.unwrap(), "2");
        assert_eq!(pool.allocate(2).await.unwrap(), "1");

        let (pool, cities, _) = fixture(Some(vec![10, 10, 8]));
        for _ in 0..12 {
            pool.allocate(1).await.unwrap();
        }
        set_players(&cities, 0, 10);
        set_players(&cities, 1, 2);
        assert_eq!(pool.allocate(1).await.unwrap(), "2");
        set_players(&cities, 0, 9);
        assert_eq!(pool.allocate(1).await.unwrap(), "1");
    }

    fn connection(host: &str, port: u16) -> Connection {
        Connection {
            host: host.into(),
            port,
            session_id: None,
        }
    }

    fn load(local: f64, remote: f64) -> impl Fn() -> BoxFuture<'static, Result<(f64, f64), DeployError>> + Send + Sync {
        move || Box::pin(async move { Ok((local, remote)) })
    }

    #[tokio::test]
    async fn the_router_sends_a_party_to_the_less_occupied_host_and_reserves() {
        let (main, worker) = (connection("main", 8777), connection("worker", 8762));
        let router = CityRouter::new(load(27.0, 2.0), || 0);
        assert_eq!(
            router
                .launch(4, async { Ok(main.clone()) }, async { Ok(Some(worker.clone())) }, 28.0)
                .await
                .unwrap(),
            worker
        );

        let router = Arc::new(CityRouter::new(load(27.0, 27.0), || 0));
        let (release, gate) = tokio::sync::oneshot::channel::<()>();
        let first = {
            let (router, main, worker) = (router.clone(), main.clone(), worker.clone());
            tokio::spawn(async move {
                router
                    .launch(
                        1,
                        async move {
                            gate.await.ok();
                            Ok(main)
                        },
                        async move { Ok(Some(worker)) },
                        28.0,
                    )
                    .await
            })
        };
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(
            router
                .launch(1, async { Ok(main.clone()) }, async { Ok(Some(worker.clone())) }, 28.0)
                .await
                .unwrap(),
            worker
        );
        assert!(
            router
                .launch(1, async { Ok(main.clone()) }, async { Ok(Some(worker.clone())) }, 28.0)
                .await
                .is_err()
        );
        release.send(()).unwrap();
        first.await.unwrap().unwrap();

        let router = CityRouter::new(load(10.0, 0.0), || 0);
        assert_eq!(
            router
                .launch(1, async { Ok(main.clone()) }, async { Ok(None) }, 28.0)
                .await
                .unwrap(),
            main
        );
        let router = CityRouter::new(load(28.0, 28.0), || 0);
        let refused = router
            .launch(
                1,
                async { Err(DeployError::failed("called")) },
                async { Err(DeployError::failed("called")) },
                28.0,
            )
            .await;
        assert_eq!(refused.unwrap_err(), DeployError::Capacity(Reason::Hunts));
    }

    #[tokio::test]
    async fn landing_reservations_expire_and_failed_launches_release_capacity() {
        let now = Arc::new(AtomicI64::new(0));
        let n = now.clone();
        let (main, worker) = (connection("main", 8777), connection("worker", 8762));
        let router = CityRouter::new(load(27.0, 28.0), move || n.load(Ordering::SeqCst));
        let failed = router
            .launch(
                1,
                async { Err(DeployError::failed("startup")) },
                async { Ok(Some(worker.clone())) },
                28.0,
            )
            .await;
        assert_eq!(failed.unwrap_err(), DeployError::failed("startup"));
        assert_eq!(
            router
                .launch(1, async { Ok(main.clone()) }, async { Ok(Some(worker.clone())) }, 28.0)
                .await
                .unwrap(),
            main
        );
        now.store(45_001, Ordering::SeqCst);
        assert_eq!(
            router
                .launch(1, async { Ok(main.clone()) }, async { Ok(Some(worker.clone())) }, 28.0)
                .await
                .unwrap(),
            main
        );
    }
}
