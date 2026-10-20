import assert from 'node:assert/strict';
import {test} from 'node:test';
import {CityRouter} from '../src/controllers/cityrouter';
import {CapacityUnavailable} from '../src/controllers/capacity';
const main = {host:'main',port:8777}, worker = {host:'worker',port:8762};
test('cities send a whole party to the less occupied host', async () => {
    const router = new CityRouter(async () => ({local:27,remote:2}));
    assert.deepEqual(await router.launch(4,async()=>main,async()=>worker),worker);
});
test('city reservations prevent concurrent joins taking the last slot twice', async () => {
    let release!:()=>void;
    const gate = new Promise<void>(r=>release=r);
    const router = new CityRouter(async()=>({local:27,remote:27}));
    const first=router.launch(1,async()=>{await gate;return main;},async()=>worker);
    await new Promise(r=>setImmediate(r));
    assert.deepEqual(await router.launch(1,async()=>main,async()=>worker),worker);
    await assert.rejects(router.launch(1,async()=>main,async()=>worker),CapacityUnavailable);
    release();await first;
});
test('unavailable worker falls back to available local city',async()=>{
    const router=new CityRouter(async()=>({local:10,remote:0}));
    assert.deepEqual(await router.launch(1,async()=>main,async()=>undefined),main);
});
test('full cities refuse admission without starting anything',async()=>{
    const router=new CityRouter(async()=>({local:28,remote:28}));
    await assert.rejects(router.launch(1,async()=>{throw Error('called');},async()=>{throw Error('called');}),CapacityUnavailable);
});
test('landing reservations expire and failed launches release capacity',async()=>{
    let now=0; const router=new CityRouter(async()=>({local:27,remote:28}),()=>now);
    await assert.rejects(router.launch(1,async()=>{throw Error('startup');},async()=>worker),/startup/);
    assert.deepEqual(await router.launch(1,async()=>main,async()=>worker),main);
    now=45001;
    assert.deepEqual(await router.launch(1,async()=>main,async()=>worker),main);
});

test('native occupancy reads the last complete game driver line including arriving connections',async()=>{
 const {ParseNativeOccupancy}=await import('../src/controllers/nativeoccupancy');
 assert.equal(ParseNativeOccupancy('net driver=IpNetDriver_1 connections=35 owned=32 open=32\n'),35);
 assert.equal(ParseNativeOccupancy('net driver=IpNetDriver_1 connections=35 owned=32 open=32\nnet driver=IpNetDriver_1 connections=12 owned=11 open=11\npartial'),12);
 assert.equal(ParseNativeOccupancy('first_chance code=c0000005\n'),undefined);
});
