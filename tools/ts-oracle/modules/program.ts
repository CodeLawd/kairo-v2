import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter')
const program = () => import(pathToFileURL(join(electron, 'src/lib/program.ts')).href)

export async function nextTimeOfDayInZone(target: string, now: number, timezone: string) {
  const { nextTimeOfDay } = await program()
  const original = process.env.TZ
  try {
    process.env.TZ = timezone
    return nextTimeOfDay(target, now)
  } finally {
    if (original === undefined) delete process.env.TZ
    else process.env.TZ = original
  }
}
