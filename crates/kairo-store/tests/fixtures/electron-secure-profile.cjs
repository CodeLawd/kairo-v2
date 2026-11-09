// Run as the main script of a disposable Electron 33 bundle whose productName
// and CFBundleName are a unique name beginning with KairoMigrationFixture-.
const { app, safeStorage } = require('electron')
const { createCipheriv, randomBytes } = require('node:crypto')
const { mkdirSync, writeFileSync } = require('node:fs')
const { join } = require('node:path')

const profile = process.env.KAIRO_SECRET_TEST_PROFILE
if (!profile || !app.name.startsWith('KairoMigrationFixture-')) {
  throw new Error('A disposable fixture bundle and profile are required')
}
mkdirSync(profile, { recursive: true })
app.setPath('userData', profile)
app.setPath('sessionData', profile)
app.whenReady().then(() => {
  if (!safeStorage.isEncryptionAvailable()) {
    throw new Error('Electron fixture could not access its keychain')
  }
  mkdirSync(join(profile, 'cloud'), { recursive: true })
  for (const [keyFile, blobFile, plaintext] of [
    ['cloud/key.bin', 'cloud/refresh-token.enc', 'synthetic refresh token'],
    ['api-bible-cache.key', 'api-cache-payload.json', 'synthetic licensed verse'],
  ]) {
    const key = randomBytes(32)
    const iv = randomBytes(12)
    const cipher = createCipheriv('aes-256-gcm', key, iv)
    const ciphertext = Buffer.concat([cipher.update(plaintext, 'utf8'), cipher.final()])
    writeFileSync(join(profile, keyFile), safeStorage.encryptString(key.toString('base64')), { mode: 0o600 })
    writeFileSync(join(profile, blobFile), JSON.stringify({
      ciphertext: ciphertext.toString('base64'),
      iv: iv.toString('base64'),
      authTag: cipher.getAuthTag().toString('base64'),
    }), { mode: 0o600 })
  }
  app.exit(0)
}).catch(() => {
  console.error('Disposable Electron keychain fixture failed')
  app.exit(1)
})
