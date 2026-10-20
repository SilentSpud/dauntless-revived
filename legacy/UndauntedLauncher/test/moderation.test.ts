import {test} from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import {fetchUserInfo} from '../src/main/hostapi';
import {fixedLinkUrl,isAllowedExternalUrl} from '../src/main/links';
test('ban reasons survive authentication failure and support has a fixed Discord destination',async()=>{
  const server=http.createServer((_req,res)=>{res.writeHead(403,{'content-type':'application/json'});res.end(JSON.stringify({error:'account_banned',reason:'Repeated harassment'}));});
  server.listen(0,'127.0.0.1');await new Promise<void>(r=>server.once('listening',r));
  try {
    const result=await fetchUserInfo({host:'127.0.0.1',port:(server.address() as any).port,pin:null},'UUK_'+'a'.repeat(48));
    assert.deepEqual(result,{ok:false,error:'account_banned',detail:'Repeated harassment'});
    assert.equal(fixedLinkUrl('discord'),'https://discord.gg/dauntlessrevived');
  }finally{await new Promise<void>((r,j)=>server.close(e=>e?j(e):r()));}
});
