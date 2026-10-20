import { test } from 'node:test';
import assert from 'node:assert/strict';
import { missingDirectX, gameExitDetail } from '../src/main/windows-runtime';
test('new Windows PC detects legacy DirectX dependencies independently of modern DirectX', () => {
  assert.deepEqual(missingDirectX({WINDIR:'C:\\Windows'}, p => /XINPUT1_3/i.test(p),'win32'), ['X3DAudio1_7.dll','XAPOFX1_5.dll']);
  assert.deepEqual(missingDirectX({},()=>false,'linux'),[]);
  assert.deepEqual(missingDirectX({},()=>true,'win32'),[]);
});
test('signed and unsigned missing-DLL exits provide the same actionable message', () => {
  assert.equal(gameExitDetail(-1073741515),gameExitDetail(3221225781));
  assert.match(gameExitDetail(3221225781),/0xC0000135.*required DLL/);
  assert.match(gameExitDetail(1),/0x00000001/);
});
