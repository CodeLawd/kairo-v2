// Usage: node tools/baseline/startup.mjs /path/to/Kairo.app/Contents/MacOS/Kairo
import { spawn, execFileSync } from 'node:child_process'
import { access, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { constants } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'
import { once } from 'node:events'

const executable = process.argv[2]
if (!executable) throw new Error('Provide the packaged Kairo executable path')
await access(executable, constants.X_OK)

async function until(probe, timeoutMs, process) {
  const deadline = performance.now() + timeoutMs
  while (performance.now() < deadline) {
    if (process.exitCode !== null || process.signalCode !== null) {
      throw new Error(`Kairo exited during sampling: ${process.exitCode ?? process.signalCode}`)
    }
    const result = await probe()
    if (result !== undefined) return result
    await delay(50)
  }
  throw new Error('Kairo did not reach the expected milestone before the timeout')
}

function sample(rootPid) {
  const rows = execFileSync('ps', ['-axo', 'pid=,ppid=,rss=,%cpu='], { encoding: 'utf8' })
    .trim().split('\n').map(line => line.trim().split(/\s+/).map(Number))
  const pids = new Set([rootPid])
  let previousSize
  do {
    previousSize = pids.size
    for (const [pid, parent] of rows) if (pids.has(parent)) pids.add(pid)
  } while (pids.size !== previousSize)
  const processes = rows.filter(([pid]) => pids.has(pid))
  if (!processes.some(([pid]) => pid === rootPid)) throw new Error('Kairo root process disappeared')
  return {
    processes: processes.length,
    rssKiB: processes.reduce((sum, row) => sum + row[2], 0),
    cpuPercent: Number(processes.reduce((sum, row) => sum + row[3], 0).toFixed(1)),
  }
}

async function interactive(target) {
  const socket = new WebSocket(target.webSocketDebuggerUrl)
  const request = {
    id: 1,
    method: 'Runtime.evaluate',
    params: {
      expression: `document.visibilityState === 'visible' &&
        Array.from(document.querySelectorAll('button')).some(button =>
          !button.disabled && button.getClientRects().length &&
          /sign in|continue|get started|skip/i.test(button.innerText))`,
      returnByValue: true,
    },
  }
  try {
    await once(socket, 'open', { signal: AbortSignal.timeout(2000) })
    socket.send(JSON.stringify(request))
    const [event] = await once(socket, 'message', { signal: AbortSignal.timeout(2000) })
    return JSON.parse(event.data).result?.result?.value === true
  } finally {
    socket.close()
  }
}

for (let trial = 1; trial <= 3; trial++) {
  const profile = await mkdtemp(join(tmpdir(), 'kairo-packaged-profile-'))
  await writeFile(join(profile, 'kairo-settings.json'), JSON.stringify({
    workspace: { folder: join(profile, 'workspace') },
    usage: { shareStats: false, sendCrashReports: false },
  }))
  const start = performance.now()
  const child = spawn(executable, [
    `--user-data-dir=${profile}`, '--remote-debugging-port=0',
    '--remote-debugging-address=127.0.0.1',
  ], { stdio: ['ignore', 'ignore', 'inherit'] })
  const exited = once(child, 'exit')
  try {
    const port = await until(async () => {
      try {
        const [value] = (await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')
        return Number(value) || undefined
      } catch (error) {
        if (error.code !== 'ENOENT') throw error
      }
    }, 20000, child)
    const startupMs = await until(async () => {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, {
        signal: AbortSignal.timeout(2000),
      })
      const targets = await response.json()
      for (const target of targets) {
        if (target.type === 'page' && target.url.startsWith('file:') && await interactive(target)) {
          return Math.round(performance.now() - start)
        }
      }
    }, 30000, child)
    await delay(15000)
    const idle = []
    for (let second = 0; second < 5; second++) {
      idle.push(sample(child.pid))
      await delay(1000)
    }
    console.log(JSON.stringify({ trial, startupMs, idle }))
  } finally {
    child.kill('SIGTERM')
    await Promise.race([exited, delay(3000)])
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL')
    await exited
    await rm(profile, { recursive: true, force: true, maxRetries: 3 })
  }
}
