// Drive the Electron service using a disposable profile and a shared fixture folder.
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

const { folder, queries } = JSON.parse(readFileSync(0, 'utf8'))
const source = pathToFileURL(join(process.cwd(), 'src/main/services/lyrics/index.ts')).href
const { lyricsService } = await import(source)
Object.assign(lyricsService, { songsDir: () => folder })
try {
  lyricsService.open()
  const results = queries.map((query: string) =>
    lyricsService.search(query).map((song: { id: string }) => song.id))
  process.stdout.write(JSON.stringify(results))
} finally {
  lyricsService.close()
}
