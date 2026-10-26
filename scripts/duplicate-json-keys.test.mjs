import assert from 'node:assert/strict';
import test from 'node:test';
import { duplicateJsonKeys } from './duplicate-json-keys.mjs';

test('reports duplicate keys with both source lines, even when values match', () => {
  assert.deepEqual(duplicateJsonKeys('{\n  "disk": "System",\n  "disk": "System"\n}'), [
    { path: 'disk', line: 3, firstLine: 2 },
  ]);
});

test('checks nested objects and arrays without confusing separate object scopes', () => {
  assert.deepEqual(duplicateJsonKeys('{"items":[{"name":1,"name":2},{"name":3}],"nested":{"key":1,"key":2}}'), [
    { path: 'items[0].name', line: 1, firstLine: 1 },
    { path: 'nested.key', line: 1, firstLine: 1 },
  ]);
  assert.deepEqual(duplicateJsonKeys('{"left":{"name":1},"right":{"name":2}}'), []);
});

test('compares decoded JSON keys and ignores key-like text in values', () => {
  assert.deepEqual(duplicateJsonKeys('{"disk":1,"\\u0064isk":2}'), [{ path: 'disk', line: 1, firstLine: 1 }]);
  assert.deepEqual(duplicateJsonKeys(JSON.stringify({ text: '"disk":1,"disk":2', empty: null, list: [] })), []);
});
