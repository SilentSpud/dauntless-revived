import { execFile } from 'node:child_process';
import path from 'node:path';
import net from 'node:net';

export interface RelayPeerTuple {
  localAddress?: string;
  localPort?: number;
  remoteAddress?: string;
  remotePort?: number;
}

function endpoint(address: string | undefined, port: number | undefined): string | null {
  const ip = address?.replace(/^::ffff:/, '');
  if (!ip || net.isIP(ip) !== 4 || !ip.startsWith('127.') || !Number.isInteger(port) || port! < 1 || port! > 65535) return null;
  return `${ip}:${port}`;
}

// Match the client row, not the accepted server row owned by the launcher.
function tcpPeerPid(table: string, peer: RelayPeerTuple): number | null {
  const client = endpoint(peer.remoteAddress, peer.remotePort);
  const server = endpoint(peer.localAddress, peer.localPort);
  if (!client || !server) return -1;
  const rows = table.split(/\r?\n/).map(line => line.trim().split(/\s+/)).filter(fields =>
    fields.length === 5 && fields[0] === 'TCP' && fields[1] === client && fields[2] === server);
  // State labels are localized. The caller must keep the accepted socket alive
  // through lookup and reject closed sockets; do not compare English labels.
  if (rows.length === 0) return null;
  return rows.length === 1 && /^\d+$/.test(rows[0][4]) ? Number(rows[0][4]) : -1;
}

export function tcpTableOwnsPeer(table: string, peer: RelayPeerTuple, pid: number): boolean {
  return Number.isSafeInteger(pid) && pid > 0 && tcpPeerPid(table, peer) === pid;
}

export function createRelayOwnerCheck(options: {
  platform?: NodeJS.Platform;
  systemRoot?: string;
  readTable?: () => Promise<string>;
} = {}): (peer: RelayPeerTuple, expectedPid: number) => Promise<boolean> {
  const platform = options.platform ?? process.platform;
  const root = options.systemRoot ?? process.env.SystemRoot;
  const readTable = options.readTable ?? (() => new Promise<string>((resolve, reject) => {
    if (!root || !path.win32.isAbsolute(root) || root.startsWith('\\\\')) return reject(new Error('Windows system directory unavailable'));
    execFile(path.win32.join(root, 'System32', 'netstat.exe'), ['-ano', '-p', 'tcp'], {
      windowsHide: true, timeout: 2000, maxBuffer: 4 * 1024 * 1024, encoding: 'utf8',
    }, (error, stdout) => error ? reject(error) : resolve(stdout));
  }));
  type Pending = { peer: RelayPeerTuple; pid: number; resolve: (allowed: boolean) => void };
  let waiting: Pending[] = [];
  let active = false;
  let count = 0;
  function schedule(): void {
    if (active || waiting.length === 0) return;
    active = true;
    // Requests already waiting share one fresh snapshot. Arrivals after the
    // batch starts wait for the next snapshot; no table is cached/reused.
    setImmediate(async () => {
      const batch = waiting; waiting = [];
      let table: string | null = null;
      try { table = await readTable(); } catch { /* fail closed; errors are not retried */ }
      const owners = batch.map(item => table === null ? -1 : tcpPeerPid(table, item.peer));
      if (owners.includes(null)) {
        // Accepted sockets can precede their owner-table row. Retry missing rows
        // once, without accepting wrong owners or reusing snapshots for arrivals.
        await new Promise(resolve => setTimeout(resolve, 25));
        try {
          const retry = await readTable();
          for (let i = 0; i < batch.length; ++i) if (owners[i] === null) owners[i] = tcpPeerPid(retry, batch[i].peer);
        } catch { /* missing rows remain denied */ }
      }
      for (let i = 0; i < batch.length; ++i) batch[i].resolve(owners[i] === batch[i].pid);
      count -= batch.length;
      active = false;
      schedule();
    });
  }
  return (peer, expectedPid) => {
    if (platform !== 'win32' || count >= 64 || !Number.isSafeInteger(expectedPid) || expectedPid <= 0 ||
        !endpoint(peer.localAddress, peer.localPort) || !endpoint(peer.remoteAddress, peer.remotePort)) return Promise.resolve(false);
    ++count;
    return new Promise<boolean>(resolve => {
      waiting.push({peer: {localAddress:peer.localAddress,localPort:peer.localPort,remoteAddress:peer.remoteAddress,remotePort:peer.remotePort}, pid: expectedPid, resolve});
      schedule();
    });
  };
}
