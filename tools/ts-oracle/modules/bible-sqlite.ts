// Compare the query used by Electron with the same bundled corpus in Rust.
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { join } from 'node:path'

const electron = process.env.KAIRO_ELECTRON_DIR!
const require = createRequire(join(electron, 'package.json'))
const Database = require('better-sqlite3')
const source = readFileSync(join(electron, 'src/main/services/scripture/bible-db.ts'), 'utf8')
const search = source.match(/searchFts: this\.db\.prepare\(`([^`]+)`\)/)?.[1]
if (!search) throw new Error('Could not find the Electron Bible FTS query')

export function ftsMatches(path: string, queries: [string, string | null][]) {
  const db = new Database(path, { readonly: true, fileMustExist: true })
  try {
    const statement = db.prepare(search)
    return queries.map(([query, translation]) => statement.all(query, translation, translation))
  } finally {
    db.close()
  }
}
