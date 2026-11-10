'use strict'
// Everything the stubs were asked to do, in order.
const g = globalThis
g.__orch ??= { calls: [], cards: [], outputs: [], overlay: undefined }
module.exports = g.__orch
