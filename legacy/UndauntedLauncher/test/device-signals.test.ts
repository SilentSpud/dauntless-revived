import { test } from 'node:test';
import assert from 'node:assert/strict';
import {collectDeviceSignals,normalizeDeviceSignals} from '../src/main/device-signals';

const uuid='12345678-1234-1234-1234-123456789abc',tpm='ab'.repeat(32);
test('device signals normalize known formats and discard firmware placeholders',()=>{
  assert.deepEqual(normalizeDeviceSignals({smbiosUuid:' '+uuid.toUpperCase()+'\r\n',tpmEkSha256:tpm.toUpperCase()}),{smbiosUuid:uuid,tpmEkSha256:tpm});
  for(const bad of ['00000000-0000-0000-0000-000000000000','ffffffff-ffff-ffff-ffff-ffffffffffff','03000200-0400-0500-0006-000700080009','Default string','To Be Filled By O.E.M.','unknown','not-a-uuid'])assert.deepEqual(normalizeDeviceSignals({smbiosUuid:bad}),{});
  for(const bad of ['0'.repeat(64),'f'.repeat(64),'g'.repeat(64),'ab',tpm+'0'])assert.deepEqual(normalizeDeviceSignals({tpmEkSha256:bad}),{});
});
test('non-Windows collection never executes a process',async()=>{
  assert.deepEqual(await collectDeviceSignals('linux',()=>{throw Error('must not execute')}),{});
});
test('Windows queries are fixed, hidden, bounded and normalized',async()=>{
  let calls=0;
  const result=await collectDeviceSignals('win32',(exe,args,options,done)=>{
    calls++;
    assert.equal(exe,'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe');
    assert.deepEqual(args.slice(0,4),['-NoLogo','-NoProfile','-NonInteractive','-Command']);
    assert.deepEqual(options,{encoding:'utf8',timeout:4000,maxBuffer:4096,windowsHide:true,shell:false});
    assert.doesNotMatch(args[4],/RunAs|Start-Process|Set-Tpm|Initialize-Tpm/);
    const isTpm=args[4].includes('Get-TpmEndorsementKeyInfo');
    if(isTpm)assert.match(args[4],/-HashAlgorithm Sha256/);
    done(null,(isTpm?tpm:uuid)+'\r\n');
  },'C:\\Windows');
  assert.equal(calls,2);assert.deepEqual(result,{smbiosUuid:uuid,tpmEkSha256:tpm});
});
test('unavailable or timed-out TPM preserves UUID without exposing error output',async()=>{
  assert.deepEqual(await collectDeviceSignals('win32',(_exe,args,_options,done)=>{
    if(args[4].includes('Get-Tpm'))done(Error('private diagnostic'),tpm);else done(null,uuid);
  }),{smbiosUuid:uuid});
});
test('process failures and oversized output produce no signals',async()=>{
  assert.deepEqual(await collectDeviceSignals('win32',()=>{throw Error('private diagnostic')}),{});
  assert.deepEqual(await collectDeviceSignals('win32',(_exe,_args,_options,done)=>done(null,'x'.repeat(4097))),{});
});
