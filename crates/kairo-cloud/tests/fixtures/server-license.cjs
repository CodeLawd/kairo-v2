// Exercise the unchanged Nest service with an in-memory fixture for MongoDB.
const path = require('node:path')
const root = process.argv[2]
process.env.TSX_TSCONFIG_PATH = path.join(root, 'server/tsconfig.json')
require('node:module').createRequire(path.join(root, 'package.json'))('tsx/cjs')
const { LicensingService } = require(path.join(root, 'server/src/licensing/licensing.service.ts'))

const orgId = '0123456789abcdef01234567'
const deviceId = 'fixture-booth'
let config
const row = {
  orgId, status: 'active', expiresAt: null, features: ['presentation', 'scripture'],
  maxDevices: 3, maxTranscriptionSessions: 3, transcriptionMinutes: 600, managedRequests: 5000,
  devices: [{ deviceId, name: 'Fixture', activatedAt: '2026-01-01', lastSeenAt: '2026-01-01' }],
  usage: { period: '2026-01', transcriptionMinutes: 0, managedRequests: 0 },
}
const db = { collection(name) {
  if (name === 'memberships') return { findOne: async () => ({ role: 'owner' }) }
  if (name === 'church_licenses') return { findOne: async () => row, updateOne: async () => ({}) }
  if (name === 'licensing_audit') return { insertOne: async () => ({}) }
  if (name === 'licensing_config') return {
    findOne: async () => config,
    createIndex: async () => {},
    updateOne: async (_, update) => {
      config = update.$setOnInsert
      return { upsertedCount: 1 }
    },
  }
  throw new Error('Unexpected database collection: ' + name)
}}
const service = new LicensingService(db, { vaultEncryptionKey: 'synthetic-license-fixture' })
service.snapshot({ orgId, deviceId, sub: orgId }).then(
  snapshot => process.stdout.write(JSON.stringify(snapshot)),
  error => { console.error(error); process.exitCode = 1 },
)
