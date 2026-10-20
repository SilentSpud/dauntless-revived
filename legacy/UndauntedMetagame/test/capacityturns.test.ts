import { test } from 'node:test';
import assert from 'node:assert/strict';
import { CapacityTurns } from '../src/controllers/capacityturns';

test('oldest capacity retry blocks later regional launches including while in flight', () => {
    const q = new CapacityTurns(60000);
    const active = new Set(['a','b','c']);
    assert.equal(q.start('a','ger',0,active),true);
    q.finish('a',true,0);
    assert.equal(q.start('b','ger',1,active),false);
    assert.equal(q.start('c','us',1,active),true);
    assert.equal(q.start('a','ger',2,active),true);
    assert.equal(q.start('b','ger',2,active),false);
    q.finish('a',false,3);
    assert.equal(q.start('b','ger',3,active),true);
});

test('original queue age wins over first allocation attempt and survives retries', () => {
    const q = new CapacityTurns(60000);
    const active = new Set(['older','newer','last']);
    assert.equal(q.start('newer','eu',100,active,20),true);
    q.finish('newer',true,101);
    assert.equal(q.start('older','eu',102,active,10),true);
    q.finish('older',true,103);
    assert.equal(q.start('newer','eu',104,active,0),false);
    assert.equal(q.start('last','eu',105,active,30),false);
    assert.equal(q.start('older','eu',106,active,1000),true);
    q.finish('older',false,107);
    assert.equal(q.start('last','eu',108,active,30),false);
    assert.equal(q.start('newer','eu',109,active,20),true);
});

test('equal-age candidates keep registration order and an in-flight attempt never expires', () => {
    const q = new CapacityTurns(10);
    const active = new Set(['a','b']);
    assert.equal(q.start('a','eu',0,active,0),true);
    assert.equal(q.start('b','eu',20,active,0),false);
    assert.equal(q.start('a','eu',20,active,0),false);
    q.finish('a',true,20);
    assert.equal(q.start('b','eu',21,active,0),false);
    q.remove('a');
    assert.equal(q.start('b','eu',22,active,0),true);
});

test('an older candidate arriving during an allocation waits for its result', () => {
    const q = new CapacityTurns(60000);
    const active = new Set(['older','newer']);
    assert.equal(q.start('newer','eu',100,active,20),true);
    assert.equal(q.start('older','eu',101,active,10),false);
    q.finish('newer',true,102);
    assert.equal(q.start('newer','eu',103,active,20),false);
    assert.equal(q.start('older','eu',104,active,10),true);
});

test('cancelled and disconnected waiters cannot block the next hunt', () => {
    for (const cancel of [true,false]) {
        const q = new CapacityTurns(60000);
        const active = new Set(['a','b']);
        q.start('a','ger',0,active);
        q.finish('a',true,0);
        assert.equal(q.start('b','ger',1,active),false);
        if(cancel) active.delete('a');
        assert.equal(q.start('b','ger',cancel ? 2 : 60001,active),true);
    }
});
