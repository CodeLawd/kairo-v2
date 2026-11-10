// Electron-as-Node oracle: the real orchestrator, detector and wiring on a
// virtual clock, with recording stubs for every service. Reads scenarios
// on stdin; writes, per scenario, the cards and the engine/program calls.
import { readFileSync } from 'node:fs'
import { mock } from 'node:test'
import { pathToFileURL } from 'node:url'
import { join } from 'node:path'

const root = process.cwd()
const load = (file: string) => import(pathToFileURL(join(root, file)).href)
delete process.env.KAIRO_STT_REPLAY_PATH
const { makeOverlayOutput } = await load('src/lib/overlay-defaults.ts')
const screen = makeOverlayOutput('screen', 'screen', { enabled: true })
const log = (await import('./stubs/log.cjs')).default
log.outputs = [screen]
log.overlay = { outputs: [screen], showTranslation: true }
const { calls, cards } = log
const { sttService } = (await import('./stubs/stt.cjs')).default
const { programService } = (await import('./stubs/program-service.cjs')).default

const flush = async () => { for (let i = 0; i < 50; i++) await Promise.resolve() }
const { scenarios } = JSON.parse(readFileSync(0, 'utf8'))
const out: unknown[] = []
mock.timers.enable({ apis: ['Date', 'setTimeout', 'setInterval'], now: 1_000_000 })
const { orchestrator } = await load('src/main/orchestrator.ts')
for (const scenario of scenarios) {
  calls.length = 0
  cards.length = 0
  programService.reset()
  await orchestrator.start({
    audioDeviceId: 'default', sttProvider: 'deepgram', sttApiKey: 'kairo-managed', sttLanguage: 'en-US',
    llmProvider: 'deepseek', llmApiKey: 'kairo-managed', scriptureTranslation: 'KJV',
    autoMode: scenario.autoMode, confidenceThreshold: 0.7, autoPresentDelaySec: 1,
  })
  let n = 0
  for (const step of scenario.steps) {
    if (step.say !== undefined) {
      sttService.emitFinal({ id: `final-${n++}`, text: step.say, words: [], timestamp: Date.now(), duration: 1, isFinal: true })
      await flush()
    } else if (step.wait !== undefined) {
      for (let i = 0; i < step.wait; i++) { mock.timers.tick(1); await flush() }
    } else if (step.approve !== undefined) {
      const ids = [...(orchestrator as any).pendingSuggestions.keys()]
      await orchestrator.approveSuggestion(ids[step.approve])
      await flush()
    } else if (step.auto !== undefined) {
      orchestrator.setAutoMode(step.auto)
    }
  }
  await orchestrator.stop()
  out.push({ cards: [...cards], calls: calls.map((c) => c.map(String)) })
}
process.stdout.write(JSON.stringify(out))
process.exit(0)
