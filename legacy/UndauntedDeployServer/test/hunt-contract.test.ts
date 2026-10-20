import './setup';
import { RemoveDeployTestDir } from './deployenv';
import assert from 'node:assert/strict';
import { after, test } from 'node:test';
import { EventEmitter } from 'node:events';
import PlayerTable from '../src/vendor/player_hunts_table.json';
import MatchmakerTable from '../src/vendor/matchmaker_hunts_table.json';
import { GetTrainingDojoConnectionDetails, ResetGameserversForTests, Startup, StartupGameserverWithHuntIdAndPlayers, UseProcessFunctionsForTests } from '../src/controllers/gameservers';

let args: string[] = [];
UseProcessFunctionsForTests({ Spawn: (_command, argv) => {
    args = argv;
    return Object.assign(new EventEmitter(), { pid: 42000, unref() {} }) as any;
}, IsAlive: () => true });
after(() => { UseProcessFunctionsForTests({}); ResetGameserversForTests(); RemoveDeployTestDir(); });

test('every enabled catalog hunt supplies a map, matchmaker hunt and the complete party roster', async () => {
    let checked = 0;
    for (const [hunt, row] of Object.entries(PlayerTable[0].Rows) as [string, any][]) {
        const ids = row.MatchmakerHuntIDs.map((r: any) => r.RowName);
        if (!ids.length || ids.some((id: string) => !(id in MatchmakerTable[0].Rows))) continue;
        ResetGameserversForTests();
        await Startup();
        await StartupGameserverWithHuntIdAndPlayers(hunt, ['UID-a', 'UID-b']);
        assert.match(args[2], /^\/Game\//, hunt);
        assert.ok(args[4] && args[4] !== 'NO_MM_HUNTID', hunt);
        assert.equal(args[5], `UID-a:${hunt},UID-b:${hunt}`, hunt);
        checked++;
    }
    assert.ok(checked > 100, `only ${checked} catalog hunts checked`);
});

test('concurrent Training requests share a world with catalog rules and no fixed player roster', async () => {
    ResetGameserversForTests();
    const replies = await Promise.all(Array.from({length: 12}, () => GetTrainingDojoConnectionDetails()));
    assert.equal(new Set(replies.map(reply => reply.port)).size, 1);
    assert.match(args[2], /\?game=.*BPGM_ArchonTrainingGrounds_C\?MaxPlayers=12$/);
    assert.equal(args[4], 'CR19_MatchmakerHunt_ShatteredIsles_TrainingDojo');
    assert.equal(args[5], 'NO_EXPECTED_PLAYERS');
});
