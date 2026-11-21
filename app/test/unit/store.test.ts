import { test } from 'node:test';
import assert from 'node:assert/strict';
import { asideName, FILE_CAP } from '../../src/main/store.ts';

test('a broken file is set aside under its name plus .broken- and the UTC time to the second', () => {
  assert.equal(asideName('/A/settings.json', new Date(Date.UTC(2026, 10, 20, 9, 5, 7, 450))), '/A/settings.json.broken-20261120T090507Z');
});

test('the app reads back no file over 1 MiB', () => {
  assert.equal(FILE_CAP, 1024 * 1024);
});
