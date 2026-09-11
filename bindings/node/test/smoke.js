'use strict';

// Minimal smoke test for CI release builds. Exercises the surface — basic
// vector graphics + serialization — against the published-shape package. The
// full surface is covered by test/run.js, not here.
//
// Verifies the published-shape package can locate + load the native library and
// round-trip a document. Exits non-zero on any failed assertion.

const assert = require('assert');
const rp = require('../lib');

const doc = new rp.Document();
doc.addPage().setFillRgb(0.1, 0.2, 0.8).rect(72, 72, 200, 100).fill();
const data = doc.toBytes();
doc.close();

assert.ok(Buffer.isBuffer(data), 'toBytes must return a Buffer');
assert.deepStrictEqual(data.subarray(0, 5), Buffer.from('%PDF-'), 'output must start with %PDF-');

console.log('smoke OK', rp.version(), data.length, 'bytes');
