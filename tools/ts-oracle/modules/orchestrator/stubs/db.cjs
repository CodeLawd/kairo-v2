'use strict'
const log = require('./log.cjs')
exports.store = { get: (key) => (key === 'overlay' ? log.overlay : undefined) }
