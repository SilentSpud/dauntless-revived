import {test} from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import {checkDeviceSignals} from '../src/main/hostapi';

test('device check supports legacy hosts and optional hardware, but rejects restrictions and errors',async()=>{
  let status=204,body:unknown={},received='';
  const server=http.createServer((req,res)=>{
    assert.equal(req.url,'/undaunted/api/DeviceSignals');
    assert.equal(req.headers['x-undaunted-user-api-key'],'test-key');
    received='';req.on('data',chunk=>received+=chunk);
    req.on('end',()=>{res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify(body));});
  });
  await new Promise<void>(resolve=>server.listen(0,'127.0.0.1',resolve));
  const ep={host:'127.0.0.1',port:(server.address() as {port:number}).port,pin:null};
  try{
    assert.deepEqual(await checkDeviceSignals(ep,'test-key',{}),{ok:true});assert.equal(received,'{}');
    status=404;assert.deepEqual(await checkDeviceSignals(ep,'test-key',{}),{ok:true});
    status=503;body={error:'device_signals_unavailable'};assert.deepEqual(await checkDeviceSignals(ep,'test-key',{}),{ok:true});
    body={error:'other'};assert.deepEqual(await checkDeviceSignals(ep,'test-key',{}),{ok:false,error:'server_error'});
    status=403;body={error:'device_banned',reason:'Appeal with support'};
    assert.deepEqual(await checkDeviceSignals(ep,'test-key',{}),{ok:false,error:'account_banned',detail:'Device restriction: Appeal with support'});
    status=429;assert.deepEqual(await checkDeviceSignals(ep,'test-key',{}),{ok:false,error:'rate_limited'});
  }finally{await new Promise<void>(resolve=>server.close(()=>resolve()));}
});
