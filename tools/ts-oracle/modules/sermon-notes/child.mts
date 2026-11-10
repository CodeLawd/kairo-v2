// Runs under ../sermon-notes.ts with Electron stubbed.
import { readFileSync, writeFileSync } from 'node:fs'

const electron = process.env.KAIRO_ELECTRON_DIR!
const { analyzeScriptureReferences, readSermonDocument } = await import(`${electron}/src/main/services/scripture/sermon-plans.ts`)
const input = JSON.parse(readFileSync(process.env.KAIRO_ORACLE_INPUT!, 'utf8'))
const realNow = Date.now
// Item ids carry `Date.now()`: pin it for the scan only.
function analyze(text: string, translation: string) {
  Date.now = () => 1_700_000_000_000
  try {
    return analyzeScriptureReferences(text, translation)
  } finally {
    Date.now = realNow
  }
}
let out: unknown
if (input.analyze) {
  out = input.analyze.map(([text, translation]: [string, string]) => analyze(text, translation))
} else {
  out = []
  for (const file of input.read as string[]) {
    try {
      const { text, html } = await readSermonDocument(file)
      ;(out as unknown[]).push({ text, html, ...analyze(text, input.translation) })
    } catch (error) {
      ;(out as unknown[]).push({ error: (error as Error).message })
    }
  }
}
writeFileSync(process.env.KAIRO_ORACLE_OUTPUT!, JSON.stringify(out))
