import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter')
const lib = (name: string) => import(pathToFileURL(join(electron, 'src/lib', `${name}.ts`)).href)

export async function parseBundle(raw: string, now: number) {
  const { parseKairoBundle } = await lib('kairo-bundle')
  const original = Date.now
  try {
    Date.now = () => now
    return { result: parseKairoBundle(raw) }
  } catch (error) {
    return { error: (error as Error).message }
  } finally {
    Date.now = original
  }
}
