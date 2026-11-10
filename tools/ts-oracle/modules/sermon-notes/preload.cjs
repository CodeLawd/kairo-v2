// Loaded with --require: redirects `electron` and `electron-log` to the
// launch stubs. Unlike electron-launch/preload.cjs it leaves `Date.now`
// alone; the child pins time only around the reference scan.
'use strict'
const Module = require('node:module')
const path = require('node:path')
const launch = path.join(__dirname, '../electron-launch')
const stubs = {
  electron: path.join(launch, 'electron-stub.cjs'),
  'electron-log': path.join(launch, 'log-stub.cjs'),
  'electron-log/main': path.join(launch, 'log-stub.cjs'),
}
const resolve = Module._resolveFilename
Module._resolveFilename = function (request, ...rest) {
  return stubs[request] ?? resolve.call(this, request, ...rest)
}
