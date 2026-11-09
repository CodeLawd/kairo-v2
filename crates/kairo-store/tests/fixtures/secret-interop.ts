// Electron's persisted payload format, using a synthetic fixture key.
import { createCipheriv, createDecipheriv, randomBytes } from 'node:crypto'
import { readFileSync } from 'node:fs'

type StoredBlob = { ciphertext: string; iv: string; authTag: string }
const key = Buffer.alloc(32, 7)
const input = readFileSync(0, 'utf8')
if (process.argv[2] === 'encrypt') {
  const iv = randomBytes(12)
  const cipher = createCipheriv('aes-256-gcm', key, iv)
  const ciphertext = Buffer.concat([cipher.update(input, 'utf8'), cipher.final()])
  const blob: StoredBlob = {
    ciphertext: ciphertext.toString('base64'),
    iv: iv.toString('base64'),
    authTag: cipher.getAuthTag().toString('base64'),
  }
  process.stdout.write(JSON.stringify(blob))
} else {
  const blob: StoredBlob = JSON.parse(input)
  const decipher = createDecipheriv('aes-256-gcm', key, Buffer.from(blob.iv, 'base64'))
  decipher.setAuthTag(Buffer.from(blob.authTag, 'base64'))
  process.stdout.write(Buffer.concat([
    decipher.update(Buffer.from(blob.ciphertext, 'base64')),
    decipher.final(),
  ]))
}
