import {test} from 'node:test';
import assert from 'node:assert/strict';
import {RegionProbes} from '../src/controllers/regionprobes';
test('only configured enabled regions advertise probes; invalid ports are refused',()=>{
    const names=['US_REGION','GERMANY_REGION','REGION_MAIN_PROBE_HOST','REGION_US_PROBE_HOST','REGION_GER_PROBE_HOST','REGION_US_PROBE_PORT'];
    const old=names.map(n=>process.env[n]);
    try {
        names.forEach(n=>delete process.env[n]);
        process.env.REGION_US_PROBE_HOST='us.example.com';
        assert.deepEqual(RegionProbes(),[]);
        process.env.US_REGION='1';
        assert.deepEqual(RegionProbes(),[{region:'us',host:'us.example.com',port:443}]);
        process.env.REGION_US_PROBE_PORT='8777';assert.equal(RegionProbes()[0].port,8777);
        process.env.REGION_US_PROBE_PORT='65536';assert.throws(RegionProbes,/Invalid REGION_US_PROBE/);
    } finally {names.forEach((n,i)=>{if(old[i]===undefined)delete process.env[n];else process.env[n]=old[i];});}
});

test('retired OCE never advertises a probe even with stale flags',()=>{
 process.env.AUS_REGION='1';process.env.REGION_AUS_PROBE_HOST='retired.example.com';
 try{assert.ok(!RegionProbes().some(r=>String(r.region)==='aus'));}
 finally{delete process.env.AUS_REGION;delete process.env.REGION_AUS_PROBE_HOST;}
});
