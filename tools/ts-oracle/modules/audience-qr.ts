// Prompt 046's join-card QR codes. Electron's `qrcode` package with the
// options `sources/kairo.ts` passes to `toDataURL`.
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { join, resolve } from 'node:path'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || fileURLToPath(new URL('../../../../kairo-presenter', import.meta.url)))
const qrcode = createRequire(join(electron, 'package.json'))('qrcode')
const OPTIONS = { width: 384, margin: 2, errorCorrectionLevel: 'M' }

/** The module grid: its width and one `0`/`1` per module, row by row. */
export function matrix(url: string): { size: number; bits: string } {
  const qr = qrcode.create(url, OPTIONS)
  return { size: qr.modules.size, bits: Array.from(qr.modules.data as Uint8Array, (b: number) => (b ? '1' : '0')).join('') }
}

/** The data URL the join card shows. */
export function dataUrl(url: string): Promise<string> {
  return qrcode.toDataURL(url, OPTIONS)
}
