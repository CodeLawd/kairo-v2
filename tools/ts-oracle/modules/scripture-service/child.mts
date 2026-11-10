// Runs in Electron as Node (see ../scripture-service.ts). Builds or opens a
// Bible, runs each operation through Electron's ScriptureService with no API
// key, and writes the results. A throwing operation reports its message.
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const electron = process.env.KAIRO_ELECTRON_DIR!
const { BibleDatabase } = await import(`${electron}/src/main/services/scripture/bible-db.ts`)
const { ScriptureService } = await import(`${electron}/src/main/services/scripture/index.ts`)
const input = JSON.parse(readFileSync(process.env.KAIRO_ORACLE_INPUT!, 'utf8'))

const db = new BibleDatabase(input.db ?? join(input.dir, 'fixture.db'))
for (const [id, name, language, isDefault, rows] of input.fixture ?? []) {
  const verses = rows.map(([bookId, chapter, verse, text]: [number, number, number, string]) => ({ bookId, chapter, verse, text }))
  db.importTranslation(id, name, language, verses, isDefault)
}
const service = new ScriptureService()
Object.assign(service, { db })

const results = []
for (const op of input.operations) {
  try {
    switch (op.op) {
      case 'search':
        results.push(await service.search(op.query, op.translation ?? undefined, ''))
        break
      case 'quote':
        results.push(service.searchLocalQuoteCandidates(op.phrase))
        break
      case 'chapterVerses':
        results.push(service.getLocalChapterVerses(op.book, op.chapter))
        break
      case 'options':
        results.push(await service.getTranslationOptions(''))
        break
      case 'status':
        results.push(service.getLocalBiblePackStatus(op.translation))
        break
      case 'timedSearch': {
        // One warmup, then `runs` timed searches; the median in milliseconds.
        await service.search(op.query, op.translation ?? undefined, '')
        const times = []
        for (let i = 0; i < op.runs; i++) {
          const start = performance.now()
          await service.search(op.query, op.translation ?? undefined, '')
          times.push(performance.now() - start)
        }
        times.sort((a, b) => a - b)
        results.push(times[Math.floor(times.length / 2)])
        break
      }
      case 'installed':
        results.push(service.listInstalledLocalBiblePacks())
        break
      default:
        throw new Error(`unknown operation ${op.op}`)
    }
  } catch (error) {
    results.push({ throw: (error as Error).message })
  }
}
db.close()
writeFileSync(process.env.KAIRO_ORACLE_OUTPUT!, JSON.stringify(results))
