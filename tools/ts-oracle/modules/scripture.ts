// Adapters for prompt 015's scripture comparisons: exports whose arguments or
// results are not plain JSON (callbacks, Sets, Maps, RegExps, a class, a
// ProseMirror document) are driven here and reduced to JSON. Each adapter
// calls the real Electron code; nothing is re-implemented.
import { createRequire } from 'node:module'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter')
const source = (path: string) => import(pathToFileURL(join(electron, path)).href)
const lib = (name: string) => source(`src/lib/${name}.ts`)
// The Electron checkout's own dependencies.
const electronRequire = createRequire(join(electron, 'package.json'))
const dependency = (name: string) => import(pathToFileURL(electronRequire.resolve(name)).href)

type Json = any
/** A scripted search: `"query|translation"` → results, or `{ throw: message }`. */
type Script = Record<string, Json>

function scriptedSearch(script: Script) {
  return async (query: string, translation: string) => {
    const hit = script[`${query}|${translation}`]
    if (hit && !Array.isArray(hit) && typeof hit.throw === 'string') throw new Error(hit.throw)
    return hit ?? []
  }
}

export async function namePatterns(): Promise<Json> {
  const { buildNamePatterns } = await lib('bible-translations')
  return buildNamePatterns().map(([pattern, id]: [RegExp, string]) => [pattern.source, id])
}

/** Which registry name patterns match each name, in sorted pattern order. */
export async function namePatternMatches(names: string[]): Promise<Json> {
  const { buildNamePatterns } = await lib('bible-translations')
  const patterns = buildNamePatterns()
  return names.map((name) => patterns.map(([pattern]: [RegExp]) => pattern.test(name)))
}

/** `getScriptureCacheNotice` with an `Error`, a string, or another value. */
export async function cacheNotice(kind: 'error' | 'string' | 'null' | 'number', message: string): Promise<Json> {
  const { getScriptureCacheNotice } = await lib('scripture-offline-state')
  const value = kind === 'error' ? new Error(message) : kind === 'string' ? message : kind === 'number' ? 42 : null
  return getScriptureCacheNotice(value)
}

export async function apiCacheMessages(): Promise<Json> {
  const policy = await source('src/main/services/scripture/api-cache-policy.ts')
  const offline = await lib('scripture-offline-state')
  return { policy: policy.API_CACHE_MESSAGES, shared: offline.API_CACHE_MESSAGES, attribution: offline.API_BIBLE_ATTRIBUTION }
}

/** Replays operations on a `ScriptureTraceStore` and reports what it holds. */
export async function traceStoreReplay(limit: number | undefined, ops: Json[]): Promise<Json> {
  const { ScriptureTraceStore } = await lib('scripture-trace-store')
  const store = new ScriptureTraceStore(limit)
  const observed: Json[] = []
  for (const op of ops) {
    if (op.op === 'start') store.start(op.input)
    else if (op.op === 'mark') store.mark(op.id, op.stage, op.at)
    else if (op.op === 'complete') store.complete(op.id, op.input)
    else if (op.op === 'annotate') store.annotate(op.id, op.fields)
    else if (op.op === 'clear') store.clear()
    else if (op.op === 'recent') observed.push(store.getRecent(op.limit).map((t: Json) => t.correlationId))
    else if (op.op === 'get') observed.push(store.get(op.id) ?? null)
    else if (op.op === 'metrics') observed.push(store.metrics(op.id) ?? null)
  }
  return { size: store.size, observed, recent: store.getRecent(1_000_000) }
}

/** `mapTextRangeToPos` on a StarterKit document built from ProseMirror JSON. */
export async function mapRange(docJson: Json, ranges: [number, number][], separator: string): Promise<Json> {
  const { getSchema, getText, getTextSerializersFromSchema } = await dependency('@tiptap/core')
  const { default: StarterKit } = await dependency('@tiptap/starter-kit')
  const { mapTextRangeToPos } = await lib('sermon-notes-review')
  const schema = getSchema([StarterKit])
  const doc = schema.nodeFromJSON(docJson)
  const text = getText(doc, { blockSeparator: separator, textSerializers: getTextSerializersFromSchema(schema) })
  return {
    text,
    ranges: ranges.map(([start, end]) => mapTextRangeToPos(doc, start, end, separator)),
  }
}

export async function reload(passages: Json[], translation: string, script: Script): Promise<Json> {
  const { reloadPassagesInTranslation } = await lib('scripture-query')
  return reloadPassagesInTranslation(passages, translation, scriptedSearch(script))
}

export async function resolveItems(items: Json[], preferred: string, script: Script, options: Json): Promise<Json> {
  const { resolveSermonPlanItems } = await lib('sermon-plan-resolve')
  return resolveSermonPlanItems(items, preferred, scriptedSearch(script), options)
}

function indexJson(index: Json): Json {
  return {
    planId: index.planId,
    ordered: index.ordered,
    expectedIndex: index.expectedIndex,
    byVerseKey: [...index.byVerseKey],
    byVerseTranslation: [...index.byVerseTranslation],
    quotable: index.quotable,
  }
}

export async function planIndex(plan: Json): Promise<Json> {
  const { buildSermonPlanIndex } = await lib('sermon-plan-match')
  return indexJson(buildSermonPlanIndex(plan))
}

export async function planReference(plan: Json | null, ref: Json, translation: string | undefined): Promise<Json> {
  const { buildSermonPlanIndex, matchPlanReference } = await lib('sermon-plan-match')
  return matchPlanReference(plan ? buildSermonPlanIndex(plan) : null, ref, translation)
}

export async function planQuote(plan: Json | null, expectedIndex: number, transcript: string, minCoverage: number | undefined): Promise<Json> {
  const { buildSermonPlanIndex, matchPlanQuote } = await lib('sermon-plan-match')
  const index = plan ? buildSermonPlanIndex(plan) : null
  if (index) index.expectedIndex = expectedIndex
  return matchPlanQuote(index, transcript, minCoverage)
}

/** Advances the expectation through `steps` of `[reference, itemId]`. */
export async function planAdvance(plan: Json, steps: [string, string | undefined][]): Promise<Json> {
  const { buildSermonPlanIndex, advancePlanExpectation } = await lib('sermon-plan-match')
  const index = buildSermonPlanIndex(plan)
  return steps.map(([reference, itemId]) => [advancePlanExpectation(index, reference, itemId), index.expectedIndex])
}

export async function suggestionSent(suggestions: Json[], sent: string[], id: string): Promise<Json> {
  const { applyOperatorSuggestionSent } = await lib('scripture-live-progress')
  const result = applyOperatorSuggestionSent(suggestions, new Set(sent), id)
  return { suggestions: result.suggestions, sent: [...result.sentSuggestionIds].sort() }
}

export async function nextAutoFollow(suggestions: Json[], active: string, armed: string, sent: string[]): Promise<Json> {
  const { nextAutoFollowSuggestion } = await lib('scripture-live-progress')
  return nextAutoFollowSuggestion(suggestions, active, armed, new Set(sent))
}

/** Threads `updatePassageFollow` through `[transcript, final]` steps. */
export async function followSequence(state: Json, steps: [string, boolean][], suggestions: Json[]): Promise<Json> {
  const { updatePassageFollow } = await lib('scripture-live-progress')
  const updates: Json[] = []
  for (const [transcript, final] of steps) {
    const update = updatePassageFollow(state, transcript, suggestions, final)
    updates.push(update)
    state = update.state
  }
  return updates
}

/** Merges suggestions one by one into an initial list. */
export async function mergeSequence(initial: Json[], incoming: Json[]): Promise<Json> {
  const { mergeScriptureSuggestion } = await lib('scripture-live-progress')
  return incoming.reduce((list: Json[], item: Json) => mergeScriptureSuggestion(list, item), initial)
}

export async function tabPush(suggestion: Json): Promise<Json> {
  const { pushScriptureFromTab } = await lib('scripture-tab-presentation')
  const calls: Json[] = []
  await pushScriptureFromTab({
    presentDirectly: async (value: Json) => { calls.push(['presentDirectly', value]) },
    register: async (value: Json) => { calls.push(['register', value]) },
    approve: async (id: string) => { calls.push(['approve', id]) },
  }, suggestion)
  return calls
}

const viewModel = () => source('src/renderer/src/components/settings/offline-bible-view-model.ts')

export async function offlineRow(row: Json): Promise<Json> {
  const vm = await viewModel()
  return {
    actions: vm.getOfflineBibleActions(row),
    labels: vm.getOfflineBibleActions(row).map((a: string) => vm.OFFLINE_BIBLE_ACTION_LABELS[a]),
    status: vm.getOfflineBibleStatusLabel(row),
    percent: vm.getOfflineBibleProgressPercent(row),
  }
}

/** Expiry text with the date part removed (it follows the machine's locale). */
export async function offlineExpiry(row: Json, now: number): Promise<Json> {
  const vm = await viewModel()
  const text: string = vm.formatOfflineBibleExpiry(row, now)
  return text.startsWith('Expires ') ? 'Expires ' : text
}

export async function offlineMerge(rows: Json[], progress: Json): Promise<Json> {
  const vm = await viewModel()
  return vm.mergeOfflineDownloadProgress(rows, progress)
}
