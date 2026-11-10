// Prompt 044's `.kairo` round trips. Electron's real `src/lib/kairo-bundle.ts`
// (the functions its transfer service writes and reads files with), with
// `Date.now` pinned while each call runs so cached results repeat.
import { fileURLToPath, pathToFileURL } from 'node:url'
import { join, resolve } from 'node:path'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || fileURLToPath(new URL('../../../../kairo-presenter', import.meta.url)))
type Json = any

async function lib(): Promise<Json> {
  return import(pathToFileURL(join(electron, 'src/lib/kairo-bundle.ts')).href)
}

function pinned<T>(now: number, run: () => T): T {
  const real = Date.now
  Date.now = () => now
  try {
    return run()
  } finally {
    Date.now = real
  }
}

/** The file text Electron's export writes for `kind`. */
export async function exportText(kind: string, name: string, songs: Json[], plan: Json, appVersion: string, now: number): Promise<string> {
  const k = await lib()
  return pinned(now, () => {
    const bundle = kind === 'setlist'
      ? k.setlistBundle(name, songs, appVersion)
      : kind === 'scripture-playlist'
        ? k.playlistBundle(plan, appVersion)
        : k.songsBundle(name, songs, appVersion)
    return k.serializeBundle(bundle)
  })
}

/** Electron reading a file: the bundle, or the error it shows. */
export async function parse(text: string, now: number): Promise<Json> {
  const k = await lib()
  return pinned(now, () => {
    try {
      return { bundle: k.parseKairoBundle(text) }
    } catch (error) {
      return { error: (error as Error).message }
    }
  })
}

/** Electron importing a file and exporting what it read, as text. */
export async function reexport(text: string, now: number): Promise<string> {
  const k = await lib()
  return pinned(now, () => k.serializeBundle(k.parseKairoBundle(text)))
}
