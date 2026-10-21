import { test } from "bun:test";
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
test('dashboard unlock renders both hosts and the CPU mean in actual client script',async()=>{
 const elements=new Map();
 const element=():any=>({value:'',textContent:'',children:[],options:[],clientWidth:0,style:{},dataset:{},classList:{add(){},remove(){},toggle(){}},setAttribute(){},addEventListener(){},querySelector:()=>null,querySelectorAll:()=>[],replaceChildren(...rows){this.children=rows;},append(...rows){this.children.push(...rows);},prepend(...rows){this.children.unshift(...rows);},getContext(){return {};}});
 const document={hidden:false,head:element(),body:element(),getElementById(id){if(!elements.has(id))elements.set(id,element());return elements.get(id);},createElement:element,querySelector:()=>null,querySelectorAll:()=>[],addEventListener(){}};
 const row=(name,cpu)=>({name,cpu,online:true,hunts:2,ramUsedMB:1024,ramTotalMB:8192,huntSampleAt:new Date().toISOString()});
 const data:any={sample:null,worker:{configured:false},error:null,logNames:[],fleet:{rows:[row('Server #1',50),row('Server #2',5)],totals:{meanCpu:27.5,hunts:4,ramUsedMB:2048,ramTotalMB:16384,ramPercent:12.5,online:2,servers:2}}};
 vm.runInNewContext(await readFile(new URL('./dashboard-client.js',import.meta.url),'utf8'),{document,window:{addEventListener(){}},setInterval(){},AbortSignal,fetch:async()=>({ok:true,json:async()=>data})});
 document.getElementById('key').value='test-owner';document.getElementById('connect').onclick();
 await new Promise(resolve=>setImmediate(resolve));
 assert.equal(elements.get('error').textContent,'');
 assert.equal(elements.get('fleetMeanCpu').textContent,'27.5%');
 assert.equal(elements.get('fleetHunts').textContent,4);
 assert.equal(elements.get('fleetRows').children.length,2);
 assert.equal(elements.get('botLinked').textContent,'—');
 assert.equal(elements.get('botIssued').textContent,'—');
 data.sample={at:new Date().toISOString(),players:[],instances:[],locations:{},history:[],discord:{at:new Date().toISOString(),linkedAccounts:7,bot:{issued:20,unused:4,redeemed:15,pending:2,revoked:1},error:null}};
 document.getElementById('key').value='test-owner';document.getElementById('connect').onclick();
 await new Promise(resolve=>setImmediate(resolve));
 assert.equal(elements.get('error').textContent,'');
 assert.equal(elements.get('botLinked').textContent,7);
 assert.equal(elements.get('botIssued').textContent,20);
 assert.equal(elements.get('botRedeemed').textContent,15);
 data.aus={configured:true,online:true,sample:{at:new Date().toISOString(),cpu:20,ramUsedMB:4096,ramTotalMB:65536,logicalCpus:24,services:{deploy:true,backend:true,allowlist:true},hunts:[{kind:'hunt',port:8700,expectedPlayers:2,startedAt:new Date().toISOString()}]}};
 document.getElementById('key').value='test-owner';
 document.getElementById('connect').onclick();
 await new Promise(resolve=>setImmediate(resolve));
 assert.equal(elements.get('ausConnection').textContent,'Connected');
 assert.equal(elements.get('ausCpu').textContent,'20.0%');
 assert.equal(elements.get('ausHuntRows').children.length,1);
 assert.equal(elements.get('ausServices').children.length,4);
});

test('overview and individual server panels remain separate in the HTML',async()=>{
 const html=await readFile(new URL('./dashboard.html',import.meta.url),'utf8');
 assert.ok(html.includes('</style></head><body>'));
 const ids=[...html.matchAll(/\bid="([^"]+)"/g)].map(m=>m[1]);assert.equal(new Set(ids).size,ids.length);
 for(const id of ['overview','server1','worker','aus','fleetPlayers','fleetCpuChart','fleetRamChart','fleetHuntChart'])assert.ok(ids.includes(id),id);
 const panels=[];let current=[];
 for(const m of html.matchAll(/<section\b[^>]*>|<\/section>/g)){
  if(m[0].startsWith('</')){assert.ok(current.length);current.pop();}
  else{const id=/\bid="([^"]+)"/.exec(m[0])?.[1];if(['overview','server1','worker','aus','backend'].includes(id)){assert.equal(current.length,0,id);panels.push(id);}current.push(id||'card');}
 }
 assert.equal(current.length,0);assert.equal(panels.length,5);
});
