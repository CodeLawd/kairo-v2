// The Electron app's real settings startup, for comparing prompt 021's port.
//
// `launch(files, readOnly)` writes `files` (name → text) into a fresh userData
// folder, runs src/main/db/index.ts in a child Node process with `electron`
// stubbed (electron-launch/), and returns every file the folder then holds.
// Time and random ids are pinned, so results repeat. A launch that throws, as
// Electron does on a corrupt or unwritable settings file, reports the error.

import { execFileSync } from 'node:child_process'
import { chmodSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = fileURLToPath(new URL('./electron-launch/', import.meta.url))

export interface LaunchResult {
  ok: boolean
  error: string | null
  files: Record<string, string>
}

export function launch(files: Record<string, string>, readOnly: string[] = []): LaunchResult {
  const root = mkdtempSync(join(tmpdir(), 'kairo-launch-'))
  const userData = join(root, 'Kairo')
  try {
    execFileSync('mkdir', ['-p', userData])
    for (const [name, text] of Object.entries(files)) writeFileSync(join(userData, name), text)
    for (const name of readOnly) chmodSync(join(userData, name), 0o444)
    const electron = process.env.KAIRO_ELECTRON_DIR ?? join(process.cwd())
    let error: string | null = null
    try {
      execFileSync(
        process.execPath,
        ['--require', join(here, 'preload.cjs'), '--import', join(here, 'register.mjs'), '--import', join(electron, 'node_modules/tsx/dist/loader.mjs'), join(here, 'child.mts')],
        { cwd: electron, env: { ...process.env, KAIRO_STUB_USER_DATA: userData, KAIRO_ELECTRON_DIR: electron }, stdio: ['ignore', 'ignore', 'pipe'] },
      )
    } catch (e) {
      const stderr = String((e as { stderr?: Buffer }).stderr ?? '')
      error = stderr.split('\n').find((line) => /Error|EACCES|EPERM/.test(line))?.trim() ?? 'launch failed'
    }
    const out: Record<string, string> = {}
    for (const name of readdirSync(userData).sort()) out[name] = readFileSync(join(userData, name), 'utf8')
    return { ok: error === null, error, files: out }
  } finally {
    try { chmodSync(join(userData, 'kairo-settings.json'), 0o644) } catch { /* absent */ }
    rmSync(root, { recursive: true, force: true })
  }
}
