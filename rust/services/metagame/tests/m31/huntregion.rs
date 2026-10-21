//! `huntregion.test.ts`: public hunt queues are partitioned by region. Node's `PartyRegion` (a party's
//! routing choice) was used nowhere in the Node metagame and has no Rust counterpart; the regions here
//! come from the saved choice, as `GetHuntRegion` reads it.

use metagame::matchmaking::region::{Region, player_region, queue_key, save_region};

use crate::common::TestApp;

#[tokio::test(flavor = "multi_thread")]
async fn public_hunts_are_partitioned_by_region_while_mixed_parties_retain_one_routing_choice() {
    let app = TestApp::start().await;
    app.exec("INSERT INTO users (userId, name, notes) VALUES ('australian', 'Aussie', 0)");
    let regions = app
        .db(|conn| {
            save_region(conn, "australian", Region::Aus)?;
            Ok((player_region(conn, "australian")?, player_region(conn, "other")?))
        })
        .await;
    // A saved AUS choice outlives the AUS host and now means US
    assert_eq!(regions, (Region::Us, Region::Main));
    assert_ne!(queue_key("patrol", Region::Main), queue_key("patrol", Region::Us));
    assert_eq!(queue_key("patrol", Region::Main), "patrol");
}
