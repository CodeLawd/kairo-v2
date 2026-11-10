// Prompt 033's scripture service comparisons. `parseReference` and
// `verseBounds` call Electron's modules in this process. `run` and `runDb`
// drive Electron's real ScriptureService and BibleDatabase in a child process
// started the way Electron's own main-process tests run (Electron as Node, so
// better-sqlite3's native module loads), against a fixture Bible built from
// the arguments or a copy of an existing bible.db. No API key is passed, so
// nothing reaches the network.
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter')
const source = (path: string) => import(pathToFileURL(join(electron, path)).href)
const child = fileURLToPath(new URL('./scripture-service/child.mts', import.meta.url))

type Json = any

export async function parseReference(input: string): Promise<Json> {
  const { parseScriptureReference } = await source('src/main/services/scripture/reference.ts')
  return parseScriptureReference(input)
}

export async function verseBounds(book: string, chapter: number, verseStart: number, verseEnd?: number): Promise<Json> {
  const { isValidScriptureReference } = await source('src/main/services/scripture/verse-bounds.ts')
  return isValidScriptureReference({ book, chapter, verseStart, verseEnd })
}

/** Runs `input` (see child.mts) in Electron's main-process environment. */
function inChild(input: Json): Json {
  const dir = mkdtempSync(join(tmpdir(), 'kairo-scripture-oracle-'))
  try {
    const inPath = join(dir, 'in.json')
    const outPath = join(dir, 'out.json')
    writeFileSync(inPath, JSON.stringify({ ...input, dir }))
    const bin = createRequire(join(electron, 'package.json'))('electron') as string
    execFileSync(bin, ['./node_modules/tsx/dist/cli.mjs', '--require', './scripts/test-setup.cjs', child], {
      cwd: electron,
      env: { ...process.env, ELECTRON_RUN_AS_NODE: '1', KAIRO_ELECTRON_DIR: electron, KAIRO_ORACLE_INPUT: inPath, KAIRO_ORACLE_OUTPUT: outPath },
      stdio: ['ignore', 'ignore', 'pipe'],
      maxBuffer: 64 * 1024 * 1024,
    })
    return JSON.parse(readFileSync(outPath, 'utf8'))
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

/** Fixture translations `[id, name, language, isDefault, [[book, chapter, verse, text], ...]]`. */
export function run(fixture: Json[], operations: Json[]): Json {
  return inChild({ fixture, operations })
}

/** Operations against a temporary copy of the bible.db at `path`. */
export function runDb(path: string, operations: Json[]): Json {
  const dir = mkdtempSync(join(tmpdir(), 'kairo-scripture-db-'))
  try {
    const copy = join(dir, 'bible.db')
    copyFileSync(path, copy)
    return inChild({ db: copy, operations })
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}
