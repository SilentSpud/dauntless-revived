import { execFile } from 'node:child_process';
import path from 'node:path';

// Client-reported signals only: neither this UUID nor the TPM public-key hash is attestation.
export interface DeviceSignals { smbiosUuid?: string; tpmEkSha256?: string }
type Run = (file: string, args: string[], options: {encoding:'utf8';timeout:number;maxBuffer:number;windowsHide:boolean;shell:false}, done: (error: Error | null, stdout: string) => void) => unknown;
const run: Run = (file,args,options,done) => execFile(file,args,options,done);
const prefix = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';try{";
const scripts = [
  prefix + "[string](Get-CimInstance -ClassName Win32_ComputerSystemProduct -ErrorAction Stop).UUID}catch{}",
  prefix + "$ek=Get-TpmEndorsementKeyInfo -HashAlgorithm Sha256 -ErrorAction Stop;if($ek.IsPresent){[string]$ek.PublicKeyHash}}catch{}",
];

export function normalizeDeviceSignals(value: DeviceSignals): DeviceSignals {
  const result: DeviceSignals = {};
  const uuid = typeof value.smbiosUuid === 'string' ? value.smbiosUuid.trim().toLowerCase() : '';
  if (/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(uuid) && !/^(?:0+|f+)$/.test(uuid.replace(/-/g,'')) && uuid !== '03000200-0400-0500-0006-000700080009') result.smbiosUuid = uuid;
  const tpm = typeof value.tpmEkSha256 === 'string' ? value.tpmEkSha256.trim().toLowerCase() : '';
  if (/^[0-9a-f]{64}$/.test(tpm) && !/^(?:0+|f+)$/.test(tpm)) result.tpmEkSha256 = tpm;
  return result;
}

export async function collectDeviceSignals(platform = process.platform, execute: Run = run, systemRoot = process.env.SystemRoot ?? 'C:\\Windows'): Promise<DeviceSignals> {
  if (platform !== 'win32') return {};
  const exe = path.win32.join(systemRoot,'System32','WindowsPowerShell','v1.0','powershell.exe');
  const read = (script: string) => new Promise<string>(resolve => {
    try {
      execute(exe,['-NoLogo','-NoProfile','-NonInteractive','-Command',script],{encoding:'utf8',timeout:4000,maxBuffer:4096,windowsHide:true,shell:false},(error,stdout) => resolve(error || typeof stdout !== 'string' || stdout.length > 4096 ? '' : stdout));
    } catch { resolve(''); }
  });
  // Independent bounded calls: missing/denied/slow TPM access must not discard a valid UUID.
  const [smbiosUuid,tpmEkSha256] = await Promise.all(scripts.map(read));
  return normalizeDeviceSignals({smbiosUuid,tpmEkSha256});
}
