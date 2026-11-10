// Prompt 037's quotation comparisons. `matchMany` runs Electron's
// `matchGlobalQuote` in this process over supplied candidates. `runDb` drives
// Electron's real FTS retrieval, rarity and scoring against a temporary copy
// of a bible.db in a child process started with Electron as Node, so
// better-sqlite3's native module loads, and reports per-search timings.
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter')
const child = fileURLToPath(new URL('./global-quote/child.mts', import.meta.url))

type Json = any

/** Cases `[text, candidates, counts | null]`. Counts give word rarity, 1000 when absent. */
export async function matchMany(cases: Json[]): Promise<Json> {
  const { matchGlobalQuote } = await import(pathToFileURL(join(electron, 'src/main/services/scripture/global-quote.ts')).href)
  return cases.map(([text, candidates, counts]: [string, Json[], Record<string, number> | null]) =>
    matchGlobalQuote(text, candidates, counts ? (word: string) => counts[word] ?? 1000 : undefined))
}

/** Each text is retrieved and scored against a copy of `path`; `possible` adds rarity. */
export function runDb(path: string, texts: string[], possible: boolean, runs: number): Json {
  const dir = mkdtempSync(join(tmpdir(), 'kairo-quote-oracle-'))
  try {
    const db = join(dir, 'bible.db')
    copyFileSync(path, db)
    const inPath = join(dir, 'in.json')
    const outPath = join(dir, 'out.json')
    writeFileSync(inPath, JSON.stringify({ db, texts, possible, runs }))
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
