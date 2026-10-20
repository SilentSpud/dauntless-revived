import {test} from 'node:test';
import assert from 'node:assert/strict';
import {Counters,COUNTER_OWNER,memberCounts} from './counter.mjs';
import {ChannelType,PermissionsBitField,PermissionFlagsBits} from 'discord.js';
function fixture() {
  let edits=0,creates=0,saves=0;
  const channels=new Map(); const members=new Map([['a',{user:{bot:false}}],['b',{user:{bot:true}}]]);
  const guild={id:'123456789012345678',memberCount:2,members:{cache:members,me:{id:'bot'},fetch:async()=>members},channels:{fetch:async id=>id?channels.get(id):channels,create:async data=>{
    creates++; const c=channel(String(creates),data.name); channels.set(c.id,c); return c;
  }}};
  const category={id:'223456789012345678',type:ChannelType.GuildCategory,guild,name:'old',permissionsFor:()=>new PermissionsBitField(PermissionFlagsBits.ManageChannels),setName:async name=>{edits++;category.name=name;}};
  channels.set(category.id,category);
  function channel(id,name) {const c={id,name,type:ChannelType.GuildVoice,parentId:category.id,setName:async name=>{edits++;c.name=name;},permissionOverwrites:{cache:new Map(),edit:async(id,values)=>{
    edits++; c.permissionOverwrites.cache.set(id,{deny:new PermissionsBitField(id===guild.id?[PermissionFlagsBits.Connect,PermissionFlagsBits.Speak]:[]),allow:new PermissionsBitField(id==='bot'?[PermissionFlagsBits.ViewChannel,PermissionFlagsBits.ManageChannels]:[])});
  }}};return c;}
  const state={version:1,guilds:{}};const service=new Counters({user:{id:'bot'}},state,async()=>{saves++;});
  const replies=[]; const interaction={user:{id:COUNTER_OWNER},guild,options:{getString:()=>category.id},deferReply:async x=>replies.push(x),editReply:async x=>replies.push(x)};
  return {service,state,guild,category,channels,members,interaction,replies,channel,get edits(){return edits;},get creates(){return creates;},get saves(){return saves;}};
}
test('counter counts bots separately and creates exactly three locked channels once',async()=>{
  const f=fixture();await f.service.configure(f.interaction);
  assert.equal(f.creates,3);assert.equal(f.category.name,'📊 Live Counter');
  assert.deepEqual([...f.channels.values()].filter(c=>c.type===ChannelType.GuildVoice).map(c=>c.name),['Bots : 1','Users : 1','Total : 2']);
  const edits=f.edits;await f.service.configure(f.interaction); assert.equal(f.creates,3);assert.equal(f.edits,edits);
  for(const c of f.channels.values()) if(c.type===ChannelType.GuildVoice) assert.ok(c.permissionOverwrites.cache.get(f.guild.id).deny.has(PermissionFlagsBits.Connect|PermissionFlagsBits.Speak));
});
test('reuses existing channel and persists restart configuration; updates changed counts only',async()=>{
  const f=fixture();f.channels.set('old',f.channel('old','Bots : 0'));await f.service.configure(f.interaction);assert.equal(f.creates,2);
  const restarted=new Counters({user:{id:'bot'}},structuredClone(f.state),async()=>{});
  const edits=f.edits;await restarted.refresh(f.guild);assert.equal(f.edits,edits);
  f.members.set('c',{user:{bot:false}});f.guild.memberCount=3;await restarted.refresh(f.guild);assert.equal(f.edits,edits+2);
  assert.deepEqual(memberCounts(f.members),{Bots:1,Users:2,Total:3});
});
test('non-owner, wrong category and missing Manage Channels are refused ephemerally',async()=>{
  for(const mode of ['owner','category','permission']) {
    const f=fixture();if(mode==='owner')f.interaction.user.id='wrong';if(mode==='category')f.category.type=ChannelType.GuildText;if(mode==='permission')f.category.permissionsFor=()=>new PermissionsBitField();
    await f.service.configure(f.interaction);assert.equal(f.creates,0);assert.equal(f.saves,0);assert.equal(f.replies[0].flags,64);
  }
});
