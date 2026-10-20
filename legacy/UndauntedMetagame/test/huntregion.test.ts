import assert from 'node:assert/strict';
import { after, test } from 'node:test';
import { PlayerRegion, PartyRegion, RegionQueueKey, SetRegionReader } from '../src/controllers/huntregion';
after(()=>SetRegionReader(()=>'main'));
test('public hunts are partitioned by region while mixed parties retain one routing choice',()=>{
    SetRegionReader(id=>id==='australian'?'aus':'main');
    assert.equal(PlayerRegion('australian'),'aus');
    assert.notEqual(RegionQueueKey('patrol','main'),RegionQueueKey('patrol','aus'));
    assert.equal(PartyRegion(['australian','other']),'mixed');
    assert.equal(PartyRegion(['australian']),'aus');
    assert.equal(PartyRegion(['other']),'main');
});
