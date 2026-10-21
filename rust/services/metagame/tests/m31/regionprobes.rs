//! `regionprobes.test.ts`: only configured, enabled regions advertise a probe endpoint.

use metagame::matchmaking::region::Settings;
use serde_json::json;
use service_kit::env::Env;

fn probes(pairs: &[(&str, &str)]) -> Result<serde_json::Value, String> {
    Settings::from_env(&Env::from_pairs(pairs.iter().copied())).probes_json()
}

#[test]
fn only_configured_enabled_regions_advertise_probes_invalid_ports_are_refused() {
    let host = ("REGION_US_PROBE_HOST", "us.example.com");
    assert_eq!(probes(&[host]), Ok(json!([])));
    assert_eq!(
        probes(&[host, ("US_REGION", "1")]),
        Ok(json!([{"region": "us", "host": "us.example.com", "port": 443}]))
    );
    assert_eq!(
        probes(&[host, ("US_REGION", "1"), ("REGION_US_PROBE_PORT", "8777")]).unwrap()[0]["port"],
        8777
    );
    let refused = probes(&[host, ("US_REGION", "1"), ("REGION_US_PROBE_PORT", "65536")]).unwrap_err();
    assert!(refused.contains("Invalid REGION_US_PROBE"), "{refused}");
}

#[test]
fn retired_oce_never_advertises_a_probe_even_with_stale_flags() {
    let list = probes(&[("AUS_REGION", "1"), ("REGION_AUS_PROBE_HOST", "retired.example.com")]).unwrap();
    assert!(!list.as_array().unwrap().iter().any(|probe| probe["region"] == "aus"));
}
