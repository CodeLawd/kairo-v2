// Loaded with --require: swaps Electron and the orchestrator's service
// singletons for recording stubs. Shared libraries, the detector, the live
// wiring, voice navigation and dispatch stay real.
'use strict'
const Module = require('node:module')
const path = require('node:path')
const launch = path.join(__dirname, '../electron-launch')
const bySpecifier = {
  electron: path.join(launch, 'electron-stub.cjs'),
  'electron-log': path.join(launch, 'log-stub.cjs'),
  'electron-log/main': path.join(launch, 'log-stub.cjs'),
}
const byPath = [
  ['/src/main/services/output/surface-manager.ts', 'surface-manager.cjs'],
  ['/src/main/services/output/program-service.ts', 'program-service.cjs'],
  ['/src/main/services/media/index.ts', 'media.cjs'],
  ['/src/main/services/scripture/index.ts', 'scripture.cjs'],
  ['/src/main/services/scripture/live-plan.ts', 'live-plan.cjs'],
  ['/src/main/services/scripture/trace.ts', 'trace.cjs'],
  ['/src/main/services/stt/index.ts', 'stt.cjs'],
  ['/src/main/services/audio/index.ts', 'audio.cjs'],
  ['/src/main/services/resilience.ts', 'resilience.cjs'],
  ['/src/main/services/usage/index.ts', 'usage.cjs'],
  ['/src/main/db/index.ts', 'db.cjs'],
]
const original = Module._resolveFilename
Module._resolveFilename = function (request, parent, ...rest) {
  if (request in bySpecifier) return bySpecifier[request]
  const resolved = original.call(this, request, parent, ...rest)
  for (const [suffix, stub] of byPath) {
    if (resolved.endsWith(suffix)) return path.join(__dirname, 'stubs', stub)
  }
  if (resolved.endsWith('/src/main/services/scripture/bible-db.ts') && parent?.filename?.endsWith('/src/main/orchestrator.ts')) {
    return path.join(__dirname, 'stubs', 'bible-db.cjs')
  }
  return resolved
}
process.env.KAIRO_STUB_USER_DATA ??= require('node:os').tmpdir()
