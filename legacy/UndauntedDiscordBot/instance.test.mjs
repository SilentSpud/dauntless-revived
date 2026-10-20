import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {acquireInstance} from './instance.mjs';
test('a second instance cannot use the same persistent key state',async()=>{
  const dir=mkdtempSync(join(tmpdir(),'bot-lock-')),file=join(dir,'lock');
  try {const release=await acquireInstance(file);await assert.rejects(acquireInstance(file),/running/);await release();await (await acquireInstance(file))();}
  finally {rmSync(dir,{recursive:true,force:true});}
});
