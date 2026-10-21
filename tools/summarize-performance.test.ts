import { test } from "bun:test";
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parseSamples, statistics, summarize } from './summarize-performance.ts';

const header = 'timestamp_utc,role,cpu_core_percent,working_set_mb,players,host_cpu_percent,ram_free_mb,players_online';
test('missing readings are excluded while measured zero remains', () => {
  const rows = parseSamples('\uFEFF' + header + '\r\n2026-09-28T00:00:00Z,hunt,,900,0,,,\r\n2026-09-28T00:01:00Z,hunt,0,1000,2,,,\r\n');
  const report = summarize(rows);
  assert.deepEqual(report.groups.hunt.cpu_core_percent, { samples: 1, median: 0, p95: 0, max: 0 });
  assert.equal(report.groups.hunt.working_set_mb.max, 1000);
  assert.equal(report.groups.hunt.host_cpu_percent.samples, 0);
});
test('nearest-rank p95 and maximum do not get confused', () => {
  assert.deepEqual(statistics(Array.from({length: 100}, (_, i) => i + 1)), { samples: 100, median: 50, p95: 95, max: 100 });
});
test('rejects malformed input rather than silently reporting zero', () => {
  for (const csv of ['', 'role\nhunt', header + '\nshort', header + '\ninvalid,hunt,1,1,1,,,', header + '\n2026-09-28T00:00:00Z,hunt,NaN,1,1,,,', header + '\n2026-09-28T00:00:00Z,hunt,-1,1,1,,,']) {
    assert.throws(() => parseSamples(csv));
  }
  assert.throws(() => summarize([]));
});
test('reads the current kit header and does not expose process identifiers', () => {
  const source = readFileSync(new URL('../deploy/windows-server/DauntlessServer.Performance.ps1', import.meta.url), 'utf8');
  const actual = [...source.match(/\$script:DRPerfColumns = @\(([\s\S]*?)\r?\n\)/)[1].matchAll(/^\s*'([^']+)'/gm)].map(match => match[1]);
  const values = actual.map(name => ({timestamp_utc: '2026-09-28T00:00:00Z', role: 'host', pid: '12345', host_cpu_percent: '25', ram_free_mb: '4096'})[name] ?? '');
  const report = summarize(parseSamples(actual.join(',') + '\n' + values.join(',')));
  assert.equal(report.groups.host.host_cpu_percent.p95, 25);
  assert.ok(!JSON.stringify(report).includes('12345'));
});
test('CLI exits with an actionable usage error without inputs', () => {
  const result = spawnSync(process.execPath, [fileURLToPath(new URL('./summarize-performance.ts', import.meta.url))], {encoding: 'utf8'});
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Usage:/);
});
