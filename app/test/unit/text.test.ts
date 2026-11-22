import { test } from 'node:test';
import assert from 'node:assert/strict';
import { codePoints, plain, TEXT_CAP } from '../../src/main/text.ts';
import { seeded, hostileString } from '../support/seeded.ts';

const UNSAFE = /[\u0000-\u001f\u007f-\u009f\u200e\u200f\u202a-\u202e\u2028\u2029\u2066-\u2069]/u;

test('plain() blanks every C0 control, DEL and every C1 control with a space', () => {
  for (let c = 0; c < 0xa0; c++) {
    if (c >= 0x20 && c < 0x7f) continue;
    assert.equal(plain(`a${String.fromCharCode(c)}b`), 'a b', `U+${c.toString(16)}`);
  }
});

test('plain() blanks bidi controls and line separators, which reorder or split what the owner reads', () => {
  for (const ch of ['\u200e', '\u200f', '\u202a', '\u202b', '\u202c', '\u202d', '\u202e', '\u2066', '\u2067', '\u2068', '\u2069', '\u2028', '\u2029']) {
    assert.equal(plain(`x${ch}y`), 'x y', `U+${ch.codePointAt(0)?.toString(16)}`);
  }
});

test('plain() keeps printable text, accents, CJK, emoji and markup characters as they are', () => {
  const s = 'E-APPLIST.W7 \u2014 café 漢字 😀 [ok] <b>&amp;</b> "q" \'s\'';
  assert.equal(plain(s), s);
});

test('plain() caps the length in code points, at 2000 by default, and never splits a surrogate pair', () => {
  assert.equal(TEXT_CAP, 2000);
  assert.equal(plain('abcdef', 3), 'abc');
  assert.equal(plain('😀😀😀', 2), '😀😀');
  assert.equal([...plain('x'.repeat(TEXT_CAP + 50))].length, TEXT_CAP);
});

test('plain() replaces a lone surrogate, so what it returns is well-formed', () => {
  assert.equal(plain('a\ud800b'), 'a\ufffdb');
  assert.equal(plain('a\udfffb'), 'a\ufffdb');
});

test('generated hostile strings: plain() leaves no control, holds the cap, is well-formed, and leaves safe input unchanged', () => {
  const rng = seeded(0x5eed07);
  for (let i = 0; i < 2000; i++) {
    const s = hostileString(rng, 80);
    const cap = 1 + Math.floor(rng() * 60);
    const out = plain(s, cap);
    assert.ok(!UNSAFE.test(out), JSON.stringify(out));
    assert.ok([...out].length <= cap);
    assert.ok(out.isWellFormed());
    if (!UNSAFE.test(s) && s.isWellFormed() && [...s].length <= cap) assert.equal(out, s);
  }
});

test('codePoints counts what a reader sees as characters: an astral one is one, as jq and Python count', () => {
  assert.equal(codePoints(''), 0);
  assert.equal(codePoints('abc'), 3);
  assert.equal(codePoints('😀😀'), 2);
  assert.equal('😀😀'.length, 4);
  assert.equal(codePoints('é漢\ud800'), 3);
});
