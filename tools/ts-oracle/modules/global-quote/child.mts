// Runs in Electron as Node (see ../global-quote.ts): the worker's own
// retrieval and scoring, timed per search after `runs` warm repetitions.
import { readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { performance } from 'node:perf_hooks'

const electron = process.env.KAIRO_ELECTRON_DIR!
const Sqlite = createRequire(`${electron}/package.json`)('better-sqlite3')
const { matchGlobalQuote, retrieveGlobalQuote, wordRarity } = await import(`${electron}/src/main/services/scripture/global-quote.ts`)
const input = JSON.parse(readFileSync(process.env.KAIRO_ORACLE_INPUT!, 'utf8'))
const db = new Sqlite(input.db, { readonly: true, fileMustExist: true })
const rarity = wordRarity(db)
const search = (text: string) => matchGlobalQuote(text, retrieveGlobalQuote(db, text), input.possible ? rarity : undefined)
const results = input.texts.map(search)
const timings: number[] = []
for (let run = 0; run < input.runs; run++) {
  for (const text of input.texts) {
    const start = performance.now()
    search(text)
    timings.push(performance.now() - start)
  }
}
db.close()
writeFileSync(process.env.KAIRO_ORACLE_OUTPUT!, JSON.stringify({ results, timings }))
