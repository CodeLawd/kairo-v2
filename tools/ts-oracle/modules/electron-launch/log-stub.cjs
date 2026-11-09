// A silent electron-log.
'use strict'
const noop = () => {}
const log = { info: noop, warn: noop, error: noop, debug: noop, initialize: noop, transports: { file: {}, console: {} } }
module.exports = log
module.exports.default = log
