// Prompt 042's sermon notes comparisons. Runs Electron's real
// `readSermonDocument` (mammoth, pdf-parse, rtf-text) and
// `analyzeScriptureReferences` from `sermon-plans.ts` in a child process run
// with Electron as Node, as the app runs pdf-parse (its pdf.js 1.10 rejects
// PDFs under plain Node 24 with "bad XRef entry"), with `electron` and
// `electron-log` stubbed by `sermon-notes/preload.cjs`,
// since the module opens an electron-store. Item ids use a pinned clock.
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { createRequire } from 'node:module'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter')
const workspace = fileURLToPath(new URL('../../../', import.meta.url))
const child = fileURLToPath(new URL('./sermon-notes/child.mts', import.meta.url))
const preload = fileURLToPath(new URL('./sermon-notes/preload.cjs', import.meta.url))

type Json = any

function run(input: Json): Json {
  const dir = mkdtempSync(join(tmpdir(), 'kairo-sermon-oracle-'))
  try {
    const inPath = join(dir, 'in.json')
    const outPath = join(dir, 'out.json')
    writeFileSync(inPath, JSON.stringify(input))
    const bin = createRequire(join(electron, 'package.json'))('electron') as string
    execFileSync(bin, ['./node_modules/tsx/dist/cli.mjs', '--require', preload, child], {
      cwd: electron,
      env: { ...process.env, ELECTRON_RUN_AS_NODE: '1', KAIRO_ELECTRON_DIR: electron, KAIRO_STUB_USER_DATA: dir, KAIRO_ORACLE_INPUT: inPath, KAIRO_ORACLE_OUTPUT: outPath },
      stdio: ['ignore', 'ignore', 'pipe'],
      maxBuffer: 64 * 1024 * 1024,
    })
    return JSON.parse(readFileSync(outPath, 'utf8'))
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

/** `analyzeScriptureReferences(text, translation)` for each `[text, translation]`. */
export function analyze(cases: [string, string][]): Json {
  return run({ analyze: cases })
}

/**
 * `readSermonDocument` then `analyzeScriptureReferences` for each file, given
 * relative to the workspace root: `{ text, html, matches, items }` or `{ error }`.
 */
export function read(files: string[], translation: string): Json {
  return run({ read: files.map((f) => resolve(workspace, f)), translation })
}
