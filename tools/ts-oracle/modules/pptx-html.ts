// Prompt 045's built-in PowerPoint renderer. Runs Electron's real
// `convertPptxBuiltin` in this process. It writes `slides.html` and then
// opens a BrowserWindow, which the stubbed `electron` lacks, so it stops
// there. The media folder's file URL becomes `MEDIA` so the HTML does not
// depend on the temporary folder.
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import Module from 'node:module'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter')
const launch = fileURLToPath(new URL('./electron-launch/', import.meta.url))
const stubs: Record<string, string> = {
  electron: join(launch, 'electron-stub.cjs'),
  'electron-log': join(launch, 'log-stub.cjs'),
  'electron-log/main': join(launch, 'log-stub.cjs'),
}
const original = (Module as any)._resolveFilename
;(Module as any)._resolveFilename = function (request: string, ...rest: unknown[]) {
  if (stubs[request]) return stubs[request]
  if (request.startsWith('@shared/')) request = join(electron, 'src/lib', request.slice('@shared/'.length))
  return original.call(this, request, ...rest)
}

type Result = { html: string } | { error: string }

/** Each deck is base64 `.pptx` bytes. */
export async function slidesHtml(decks: string[]): Promise<Result[]> {
  process.env.KAIRO_STUB_USER_DATA ??= tmpdir()
  const { convertPptxBuiltin } = await import(pathToFileURL(join(electron, 'src/main/services/documents/pptx-builtin.ts')).href)
  const out: Result[] = []
  for (const deck of decks) {
    const dir = mkdtempSync(join(tmpdir(), 'kairo-pptx-oracle-'))
    try {
      const source = join(dir, 'deck.pptx')
      writeFileSync(source, Buffer.from(deck, 'base64'))
      let error = ''
      try {
        await convertPptxBuiltin(source, join(dir, 'source.pdf'), dir)
      } catch (e) {
        error = (e as Error).message
      }
      const page = join(dir, 'slides.html')
      if (existsSync(page)) {
        const media = pathToFileURL(join(dir, 'pptx', 'ppt', 'media')).href
        out.push({ html: readFileSync(page, 'utf8').split(media).join('MEDIA') })
      } else {
        out.push({ error })
      }
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  }
  return out
}
