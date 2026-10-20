import {createServer} from 'node:net';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
// OS ownership releases the lock on process death, without stale files or PID reuse.
export async function acquireInstance(stateFile) {
  const hash=createHash('sha256').update(resolve(stateFile).toLowerCase()).digest('hex');
  const server=createServer(socket=>socket.destroy());
  await new Promise((resolve,reject)=>{
    server.once('error',()=>reject(new Error('Another key bot is running or its instance lock is unavailable')));
    server.listen(process.platform==='win32' ? {path:`\\\\.\\pipe\\dauntless-key-bot-${hash}`} : {host:'127.0.0.1',port:20000+parseInt(hash.slice(0,4),16)%40000,exclusive:true},resolve);
  });
  server.unref();
  return ()=>new Promise(resolve=>server.close(resolve));
}
