//! Picking the closest hunt region (row L25): `src/main/autoregion.ts`. The host lists a probe address
//! for each region; the launcher times a TCP connect to each (twice, keeping the second) and takes the
//! fastest that answered.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyper::Method;
use serde_json::Value;
use tokio::net::TcpStream;

use super::http::{Endpoint, RequestOptions, parse_json_body, request};
use crate::core::invite::is_valid_host;

const REGIONS: [&str; 3] = ["main", "ger", "us"];
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionProbe {
    pub region: &'static str,
    pub host: String,
    pub port: u16,
}

/// How long a TCP connect takes, or None when it did not connect within 1.5 s.
pub async fn tcp_latency(probe: RegionProbe) -> Option<Duration> {
    let start = Instant::now();
    match tokio::time::timeout(PROBE_TIMEOUT, TcpStream::connect((probe.host.as_str(), probe.port))).await {
        Ok(Ok(_)) => Some(start.elapsed()),
        _ => None,
    }
}

pub type Measure = Arc<dyn Fn(RegionProbe) -> Pin<Box<dyn Future<Output = Option<Duration>> + Send>> + Send + Sync>;

/// The region whose probe answered fastest (the first reading warms up the route and is dropped).
pub async fn fastest_region(probes: &[RegionProbe], measure: Measure) -> Option<&'static str> {
    let readings = futures_util::future::join_all(probes.iter().cloned().map(|probe| {
        let measure = measure.clone();
        async move {
            let _ = measure(probe.clone()).await;
            (probe.region, measure(probe).await)
        }
    }))
    .await;
    readings
        .into_iter()
        .filter_map(|(region, latency)| latency.map(|latency| (region, latency)))
        .min_by_key(|(_, latency)| *latency)
        .map(|(region, _)| region)
}

fn region_name(value: Option<&Value>) -> Option<&'static str> {
    let name = value?.as_str()?;
    REGIONS.into_iter().find(|region| *region == name)
}

/// Asks the host for its regions' probe addresses and picks the closest; the host's own suggestion
/// when none answered.
pub async fn choose_closest_region(endpoint: &Endpoint, key: &str, measure: Option<Measure>) -> Option<&'static str> {
    let mut options = RequestOptions::get(Duration::from_secs(5), 8192);
    options.method = Method::GET;
    options.headers = vec![("x-undaunted-user-api-key", key.to_owned())];
    let response = request(endpoint, "/undaunted/api/HuntRegion", options).await.ok()?;
    if response.status != 200 {
        return None;
    }
    let body = parse_json_body(&response);
    let mut probes = vec![RegionProbe {
        region: "main",
        host: endpoint.host.clone(),
        port: endpoint.port,
    }];
    if let Some(list) = body
        .as_ref()
        .and_then(|body| body.get("probes"))
        .and_then(Value::as_array)
    {
        for candidate in list.iter().take(3) {
            let Some(region) = region_name(candidate.get("region")) else {
                continue;
            };
            let Some(host) = candidate
                .get("host")
                .and_then(Value::as_str)
                .filter(|host| is_valid_host(host))
            else {
                continue;
            };
            let Some(port) = candidate
                .get("port")
                .and_then(Value::as_u64)
                .and_then(|port| u16::try_from(port).ok())
                .filter(|port| *port >= 1)
            else {
                continue;
            };
            probes.retain(|probe| probe.region != region);
            probes.push(RegionProbe {
                region,
                host: host.to_owned(),
                port,
            });
        }
    }
    let measure = measure.unwrap_or_else(|| Arc::new(|probe| Box::pin(tcp_latency(probe))));
    match fastest_region(&probes, measure).await {
        Some(region) => Some(region),
        None => region_name(body.as_ref().and_then(|body| body.get("region"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probes() -> Vec<RegionProbe> {
        REGIONS
            .iter()
            .map(|region| RegionProbe {
                region,
                host: "example.com".into(),
                port: 443,
            })
            .collect()
    }

    fn fixed(times: [Option<u64>; 3]) -> Measure {
        Arc::new(move |probe| {
            let index = REGIONS.iter().position(|region| *region == probe.region).unwrap_or(0);
            let time = times[index].map(Duration::from_millis);
            Box::pin(async move { time })
        })
    }

    #[tokio::test]
    async fn automatic_chooses_the_fastest_reachable_region() {
        assert_eq!(
            fastest_region(&probes(), fixed([Some(200), Some(90), Some(12)])).await,
            Some("us")
        );
        assert_eq!(
            fastest_region(&probes(), fixed([None, None, Some(5)])).await,
            Some("us")
        );
        assert_eq!(
            fastest_region(&probes(), fixed([None, Some(10), None])).await,
            Some("ger")
        );
        assert_eq!(fastest_region(&probes(), fixed([None, None, None])).await, None);
    }

    #[tokio::test]
    async fn a_real_probe_connects_or_gives_up() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(
            tcp_latency(RegionProbe {
                region: "main",
                host: "127.0.0.1".into(),
                port
            })
            .await
            .is_some()
        );
        drop(listener);
        assert!(
            tcp_latency(RegionProbe {
                region: "main",
                host: "127.0.0.1".into(),
                port
            })
            .await
            .is_none()
        );
    }
}
