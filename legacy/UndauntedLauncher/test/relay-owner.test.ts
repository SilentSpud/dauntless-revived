import assert from 'node:assert/strict';
import { test } from 'node:test';
import net from 'node:net';
import { createRelayOwnerCheck, tcpTableOwnsPeer } from '../src/main/relay-owner';

const peer={localAddress:'127.0.0.1',localPort:61000,remoteAddress:'127.0.0.1',remotePort:52123};
const row=' TCP 127.0.0.1:52123 127.0.0.1:61000 ESTABLISHED 123\r\n';
test('owner check matches reverse full tuple and exact PID, never server row',()=>{
  assert.equal(tcpTableOwnsPeer(row,peer,123),true);
  for(const table of [row.replace('123\r','456\r'),row+row,
      'TCP 127.0.0.1:61000 127.0.0.1:52123 ESTABLISHED 123', row.replace('52123','52124'),row.replace('127.0.0.1:52123','127.0.0.2:52123')])
    assert.equal(tcpTableOwnsPeer(table,peer,123),false);
  assert.equal(tcpTableOwnsPeer(row,{...peer,remoteAddress:'::ffff:127.0.0.1'},123),true);
  assert.equal(tcpTableOwnsPeer(row,peer,0),false);
  assert.equal(tcpTableOwnsPeer(row.replace('ESTABLISHED','HERGESTELLT'),peer,123),true);
});
test('unsupported platforms and lookup errors fail closed; snapshots are not reused',async()=>{
  let count=0;
  const check=createRelayOwnerCheck({platform:'win32',readTable:async()=>{++count;return row;}});
  assert.equal(await check(peer,123),true);assert.equal(await check(peer,123),true);assert.equal(count,2);
  assert.equal(await createRelayOwnerCheck({platform:'linux',readTable:async()=>row})(peer,123),false);
  assert.equal(await createRelayOwnerCheck({platform:'win32',readTable:async()=>{throw Error('timeout');}})(peer,123),false);
});
test('32 simultaneous legitimate sockets share a fresh snapshot without denial',async()=>{
  let reads=0;
  const check=createRelayOwnerCheck({platform:'win32',readTable:async()=>{++reads;return Array.from({length:32},(_,i)=>row.replace('52123',String(52123+i))).join('');}});
  const results=await Promise.all(Array.from({length:32},(_,i)=>check({...peer,remotePort:52123+i},123)));
  assert.equal(results.every(Boolean),true);assert.equal(reads,1);
});
test('64 pending limit, arrivals during a read need the next snapshot, and no stale verdict survives',async()=>{
  const release:Array<(s:string)=>void>=[];
  const check=createRelayOwnerCheck({platform:'win32',readTable:()=>new Promise(resolve=>release.push(resolve))});
  const first=check(peer,123);
  await new Promise(resolve=>setImmediate(resolve));
  const later=Array.from({length:63},()=>check(peer,123));
  assert.equal(await check(peer,123),false);
  assert.equal(release.length,1);
  release[0](row);assert.equal(await first,true);
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(release.length,2);
  release[1](row.replace('123\r','456\r'));
  assert.equal((await Promise.all(later)).some(Boolean),false);
  const recovered=check(peer,123);
  await new Promise(resolve=>setImmediate(resolve));
  release[2](row);assert.equal(await recovered,true);
});
test('only missing tuples get one bounded fresh retry',async()=>{
  let reads=0;
  const appears=createRelayOwnerCheck({platform:'win32',readTable:async()=>++reads===1?'':row});
  assert.equal(await appears(peer,123),true);assert.equal(reads,2);
  reads=0;
  const absent=createRelayOwnerCheck({platform:'win32',readTable:async()=>{++reads;return '';}});
  assert.equal(await absent(peer,123),false);assert.equal(reads,2);
  reads=0;
  const wrong=createRelayOwnerCheck({platform:'win32',readTable:async()=>{++reads;return row;}});
  assert.equal(await wrong(peer,456),false);assert.equal(reads,1);
});
test('real Windows loopback socket resolves current PID and rejects another PID',{skip:process.platform!=='win32'},async()=>{
  const server=net.createServer(); let client:net.Socket|undefined; let accepted:net.Socket|undefined;
  try {
    await new Promise<void>(resolve=>server.listen(0,'127.0.0.1',resolve));
    const incoming=new Promise<net.Socket>(resolve=>server.once('connection',resolve));
    client=net.connect((server.address() as net.AddressInfo).port,'127.0.0.1');
    accepted=await incoming;
    const check=createRelayOwnerCheck();const start=performance.now();
    assert.equal(await check(accepted,process.pid),true);
    console.log(`Windows TCP owner snapshot: ${Math.round(performance.now()-start)}ms`);
    assert.equal(await check(accepted,process.pid===1?2:1),false);
  } finally {client?.destroy();accepted?.destroy();await new Promise<void>(resolve=>server.close(()=>resolve()));}
});
