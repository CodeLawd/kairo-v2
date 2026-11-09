// Loaded with --require before the launch: redirects `electron` and
// `electron-log` to stubs, and pins time and random ids so results repeat.
'use strict'
const Module = require('node:module')
const path = require('node:path')
const stubs = {
  electron: path.join(__dirname, 'electron-stub.cjs'),
  'electron-log': path.join(__dirname, 'log-stub.cjs'),
  'electron-log/main': path.join(__dirname, 'log-stub.cjs'),
}
const resolve = Module._resolveFilename
Module._resolveFilename = function (request, ...rest) {
  return stubs[request] ?? resolve.call(this, request, ...rest)
}
let next = 0
globalThis.crypto.randomUUID = () => `00000000-0000-4000-8000-${String(++next).padStart(12, '0')}`
require('node:crypto').randomUUID = globalThis.crypto.randomUUID
Date.now = () => 1_700_000_000_000
