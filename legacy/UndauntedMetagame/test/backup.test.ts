import {test} from 'node:test';
import assert from 'node:assert/strict';
import express from 'express';
import {mkdtemp, mkdir, writeFile, readFile, rm} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

process.env.DB_FILENAME = ':memory:';
const {CreateBackupRouter} = require('../src/routes/backup');
const Database = require('better-sqlite3');

test('live backup refuses proxy callers, wrong keys, traversal, duplicate copies and overwrites', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'dr-backup-route-'));
    const name = '2026-10-05_120000';
    await mkdir(path.join(root,name));
    let release!: () => void, copying!: () => void;
    const started = new Promise<void>(r=>copying=r);
    const pending = new Promise<void>(r=>release=r);
    let calls = 0;
    const app = express();app.use(express.json());
    app.use(CreateBackupRouter({root:()=>root,validKey:async(k:string)=>k==='test-key',copy:async(dest:string)=>{
        calls++;copying();await pending;await writeFile(dest,'test-copy');
    }}));
    const server=app.listen(0,'127.0.0.1');await new Promise<void>(r=>server.once('listening',r));
    const address=server.address() as {port:number};
    const post=(body:unknown,headers:Record<string,string>={})=>fetch(`http://127.0.0.1:${address.port}/internal/backup`,{method:'POST',headers:{'content-type':'application/json','x-undaunted-gameserver-apikey':'test-key',...headers},body:JSON.stringify(body)});
    try {
        assert.equal((await post({name},{'x-forwarded-for':'192.0.2.1'})).status,403);
        assert.equal((await post({name},{'x-undaunted-gameserver-apikey':'wrong'})).status,403);
        assert.equal((await post({name:'../outside'})).status,400);
        const first=post({name});await started;
        assert.equal((await post({name})).status,409);release();
        assert.equal((await first).status,200);
        assert.equal((await post({name})).status,409);
        assert.equal(calls,1);
        assert.equal(await readFile(path.join(root,name,'undaunted.db'),'utf8'),'test-copy');
    } finally {
        release();server.closeAllConnections();await new Promise<void>(r=>server.close(()=>r()));
        await rm(root,{recursive:true,force:true});
    }
});

test('backup budget limits authenticated copies before filesystem work, not rejected callers',async()=>{
    const root=await mkdtemp(path.join(os.tmpdir(),'dr-backup-limit-'));
    let copies=0;
    const app=express();app.use(express.json());
    app.use(CreateBackupRouter({root:()=>root,validKey:async(k:string)=>k==='test-key',copy:async()=>{copies++;}}));
    const server=app.listen(0,'127.0.0.1');await new Promise<void>(r=>server.once('listening',r));
    const port=(server.address() as {port:number}).port;
    const post=(name:string,key='test-key')=>fetch(`http://127.0.0.1:${port}/internal/backup`,{method:'POST',headers:{'content-type':'application/json','x-undaunted-gameserver-apikey':key},body:JSON.stringify({name})});
    try{
        for(let i=0;i<8;i++)assert.equal((await post('2026-10-05_120000','wrong')).status,403);
        for(let i=0;i<6;i++){
            const name=`2026-10-05_12000${i}`;await mkdir(path.join(root,name));
            assert.equal((await post(name)).status,200);
        }
        // This directory does not exist: 429 proves limiting precedes realpath/copy.
        const blocked=await post('2026-10-05_120006');
        assert.equal(blocked.status,429);
        assert.ok(Number(blocked.headers.get('retry-after'))>0);
        assert.equal((await blocked.json() as any).error,'rate_limited');
        assert.equal(copies,6);
    }finally{
        server.closeAllConnections();await new Promise<void>(r=>server.close(()=>r()));
        await rm(root,{recursive:true,force:true});
    }
});

test('writer-connection backup finishes while saves continue', {timeout:10000}, async () => {
    const root=await mkdtemp(path.join(os.tmpdir(),'dr-backup-writer-'));
    const db=new Database(path.join(root,'source.db'));
    try {
        db.exec('CREATE TABLE items(id INTEGER PRIMARY KEY, payload BLOB); CREATE TABLE saves(a INTEGER,b INTEGER); INSERT INTO saves VALUES(0,0)');
        const insert=db.prepare('INSERT INTO items(payload) VALUES(zeroblob(4096))');
        db.transaction(()=>{for(let i=0;i<2000;i++)insert.run();})();
        const save=db.prepare('UPDATE saves SET a=?,b=?');let changes=0;
        await db.backup(path.join(root,'copy.db'),{progress:()=>{changes++;save.run(changes,changes);return 100;}});
        assert.ok(changes>5,'multiple saves interleaved with backup steps');
        const copy=new Database(path.join(root,'copy.db'),{readonly:true});
        try {
            assert.equal(copy.pragma('integrity_check',{simple:true}),'ok');
            assert.equal(copy.prepare('SELECT count(*) AS n FROM items').get().n,2000);
            const row=copy.prepare('SELECT a,b FROM saves').get();assert.equal(row.a,row.b);assert.ok(row.a>0);
        } finally {copy.close();}
    } finally {db.close();await rm(root,{recursive:true,force:true});}
});
