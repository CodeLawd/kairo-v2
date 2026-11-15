#!/usr/bin/env node
// Lists every file under the Electron app's src/renderer with its native
// owner (prompt and crate), and whether anything imports it. Writes
// native-migration/renderer-owners.md. Exits 1 if any file has no owner.
//
//   node tools/ts-oracle/renderer-owners.cjs [path/to/kairo-presenter]

const fs = require('fs')
const path = require('path')

const root = path.resolve(__dirname, '../..')
const electron = path.resolve(process.argv[2] || path.join(root, '../kairo-presenter'))
const renderer = path.join(electron, 'src/renderer')
const src = path.join(renderer, 'src')

// First match wins. [pattern, prompt, native owner].
const RULES = [
  [/^index\.html$/, '071', 'retired: the GPUI window replaces the HTML entry (apps/kairo main.rs)'],
  [/^public\/audio\/pcm-capture\.worklet\.js$/, '038', 'kairo-stt native audio capture'],
  [/^src\/main\.tsx$/, '071', 'ui-shell (apps/kairo main.rs)'],
  [/^src\/App\.tsx$/, '071', 'ui-shell Shell'],
  [/^src\/index\.css$/, '070', 'kairo-ui-kit tokens and theme'],
  [/^src\/env\.d\.ts$/, '070', 'retired: TypeScript ambient types'],
  [/^src\/assets\//, '070', 'kairo-ui-kit brand assets'],
  [/^src\/icons\//, '070', 'kairo-ui-kit KairoIcon'],
  [/^src\/audio\//, '038', 'kairo-stt / kairo-audio device capture'],
  [/^src\/bootstrap\//, '072', 'ui-shell bootstrap and launch'],
  [/^src\/components\/account\//, '073', 'ui-account'],
  [/^src\/components\/onboarding\//, '073', 'ui-account onboarding and coach tour'],
  [/^src\/components\/audience\//, '090', 'ui-audience'],
  [/^src\/components\/brand\//, '070', 'kairo-ui-kit brand'],
  [/^src\/components\/capture\//, '091', 'ui-capture CaptureSettingsWindow'],
  [/^src\/components\/documents\//, '085', 'ui-documents'],
  [/^src\/components\/ErrorBoundary\.tsx$/, '071', 'ui-shell contained panic fallback'],
  [/^src\/components\/layout\/RailPopovers\.tsx$/, '071', 'ui-shell header popovers (audio, account); Live popover in ui-capture (091)'],
  [/^src\/components\/layout\/AppShell\.tsx$/, '071', 'ui-shell header and rail; LiveMenu in ui-capture (091)'],
  [/^src\/components\/layout\/BoothWorkspace\.tsx$/, '076', 'ui-operator workspace'],
  [/^src\/components\/layout\/ThemePicker\.tsx$/, '087', 'ui-theme ThemePicker'],
  [/^src\/components\/layout\/useSidebarWidth\.ts$/, '091', 'kairo-ui-kit page_sidebar (kairo.page-sidebar-width)'],
  [/^src\/components\/layout\//, '071', 'ui-shell'],
  [/^src\/components\/lyrics\//, '080', 'ui-lyrics (080-082)'],
  [/^src\/components\/media\//, '083', 'ui-media'],
  [/^src\/components\/operator\//, '076', 'ui-operator (076-077)'],
  [/^src\/components\/overlay\//, '075', 'ui-screens / ui-theme preview'],
  [/^src\/components\/propresenter\//, '082', 'ui-lyrics import'],
  [/^src\/components\/props\//, '088', 'ui-operator props'],
  [/^src\/components\/screens\//, '075', 'ui-screens'],
  [/^src\/components\/scripture\//, '078', 'ui-scripture (078-079)'],
  [/^src\/components\/settings\//, '074', 'ui-settings'],
  [/^src\/components\/shared\//, '070', 'kairo-ui-kit components'],
  [/^src\/components\/theme\/SetupChecklist\.tsx$/, '087', 'ui-theme panels'],
  [/^src\/components\/theme\//, '086', 'ui-theme (086-087)'],
  [/^src\/components\/tracks\//, '084', 'ui-media tracks'],
  [/^src\/components\/transcription\/Transcription\.tsx$/, '091', 'not ported: dead code; live transcript is ui-operator (076)'],
  [/^src\/components\/transfer\//, '091', 'ui-transfer'],
  [/^src\/components\/ui\//, '070', 'gpui-kit components via kairo-ui-kit'],
  [/^src\/components\/web\//, '089', 'ui-web'],
  [/^src\/hooks\/useImportRequest\.ts$/, '071', 'ui-shell ShellRequest::Import and per-page feeds'],
  [/^src\/hooks\/useUpdates\.ts$/, '071', 'ui-shell UpdateState'],
  [/^src\/hooks\/useSettings\.ts$/, '074', 'per-crate settings feeds'],
  [/^src\/hooks\/useProgramState\.ts$/, '076', 'ui-operator program feed'],
  [/^src\/hooks\/usePropTimerTick\.ts$/, '088', 'ui-operator props'],
  [/^src\/hooks\/useMultiSelect\.ts$/, '080', 'ui-lyrics / ui-documents selection'],
  [/^src\/hooks\/useDraftPresentation\.ts$/, '086', 'ui-theme drafts'],
  [/^src\/lib\/appTheme\.ts$/, '070', 'kairo-ui-kit theme'],
  [/^src\/lib\/hue\.ts$/, '070', 'kairo-ui-kit Hue'],
  [/^src\/lib\/utils\.ts$/, '070', 'retired: class-name helper'],
  [/^src\/lib\/drag\.ts$/, '070', 'kairo-ui-kit drag'],
  [/^src\/lib\/paneSizing\.ts$/, '076', 'ui-operator layout'],
  [/^src\/lib\/whats-new\.ts$/, '071', 'ui-shell whats_new'],
  [/^src\/lib\/credentials-notice\.ts$/, '071', 'ui-shell keychain notice'],
  [/^src\/lib\/clear-live-output\.ts$/, '071', 'ui-shell ShellRequest::ClearLive'],
  [/^src\/lib\/defaultSettings\.ts$/, '013', 'kairo-domain normalizers'],
  [/^src\/lib\/output-settings\.ts$/, '075', 'ui-screens model'],
  [/^src\/lib\/markSuggestionRendered\.ts$/, '078', 'ui-scripture'],
  [/^src\/stores\/useAccountStore\.ts$/, '073', 'ui-account Account'],
  [/^src\/stores\/useLicenseStore\.ts$/, '073', 'ui-account Account'],
  [/^src\/stores\/useAppStore\.ts$/, '071', 'ui-shell Shell and per-page models'],
  [/^src\/stores\/useBoothToolboxStore\.ts$/, '077', 'ui-operator toolbox'],
  [/^src\/stores\/useCaptureStore\.ts$/, '091', 'ui-capture Capture'],
  [/^src\/stores\/useLibraries\.ts$/, '080', 'ui-lyrics / ui-scripture libraries feeds'],
  [/^src\/stores\/useMediaDockStore\.ts$/, '083', 'ui-media dock'],
  [/^src\/stores\/usePassages\.ts$/, '078', 'ui-scripture passages feed'],
  [/^src\/stores\/useSetlist\.ts$/, '080', 'ui-lyrics setlists feed'],
  [/^src\/stores\/useTracksPlaybackStore\.ts$/, '084', 'ui-media tracks'],
  [/^src\/stores\/useTransfer\.ts$/, '091', 'ui-transfer model'],
]

function walk(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) walk(full, out)
    else out.push(full)
  }
  return out
}

const EXTS = ['', '.ts', '.tsx', '.js', '.css', '/index.ts', '/index.tsx']
function resolve(from, spec) {
  let base
  if (spec.startsWith('@/')) base = path.join(src, spec.slice(2))
  else if (spec.startsWith('.')) base = path.resolve(path.dirname(from), spec)
  else return null
  for (const ext of EXTS) {
    const candidate = base + ext
    if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) return candidate
  }
  return null
}

const IMPORT = /(?:import|export)\s[^'"]*?from\s*['"]([^'"]+)['"]|import\s*\(\s*['"]([^'"]+)['"]\s*\)|import\s+['"]([^'"]+)['"]|new URL\(\s*['"]([^'"]+)['"]/g

const files = walk(renderer).sort()
const importedBy = new Map(files.map((f) => [f, new Set()]))
for (const file of files) {
  if (!/\.(tsx?|js|html|css)$/.test(file)) continue
  const text = fs.readFileSync(file, 'utf8')
  const specs = []
  for (const m of text.matchAll(IMPORT)) specs.push(m[1] || m[2] || m[3] || m[4])
  // index.html's module script, CSS @import and url() assets.
  for (const m of text.matchAll(/src="([^"]+)"/g)) specs.push(m[1].startsWith('/') ? '.' + m[1] : m[1])
  for (const m of text.matchAll(/@import\s+['"]([^'"]+)['"]/g)) specs.push(m[1])
  for (const spec of specs) {
    const target = resolve(file, spec)
    if (target && importedBy.has(target)) importedBy.get(target).add(file)
  }
}
// The worklet is loaded by URL at runtime.
for (const file of files) {
  if (file.includes('/public/')) {
    const name = path.basename(file)
    for (const other of files) {
      if (/\.(tsx?)$/.test(other) && fs.readFileSync(other, 'utf8').includes(name)) importedBy.get(file).add(other)
    }
  }
}

// Reachable from the entry.
const entry = path.join(renderer, 'index.html')
const reached = new Set([entry])
const edges = new Map(files.map((f) => [f, []]))
for (const [target, sources] of importedBy) for (const s of sources) edges.get(s).push(target)
const queue = [entry]
while (queue.length) {
  for (const next of edges.get(queue.pop()) || []) {
    if (!reached.has(next)) {
      reached.add(next)
      queue.push(next)
    }
  }
}

const rows = []
const unassigned = []
for (const file of files) {
  const rel = path.relative(renderer, file).split(path.sep).join('/')
  const rule = RULES.find(([re]) => re.test(rel))
  const live = reached.has(file) ? 'yes' : 'no'
  if (!rule) unassigned.push(rel)
  rows.push(`| \`${rel}\` | ${rule ? rule[1] : '**unassigned**'} | ${rule ? rule[2] : ''} | ${live} |`)
}
const dead = rows.filter((r) => r.endsWith('| no |')).length
const out = [
  '# Renderer ownership',
  '',
  'Generated by `node tools/ts-oracle/renderer-owners.cjs`. Do not hand-edit.',
  '',
  `Files: ${files.length}. Unassigned: ${unassigned.length}. Not reachable from \`index.html\`: ${dead}.`,
  '',
  '| File | Prompt | Native owner | Reachable |',
  '|---|---|---|---|',
  ...rows,
  '',
].join('\n')
fs.writeFileSync(path.join(root, 'native-migration/renderer-owners.md'), out)
console.log(`${files.length} renderer files, ${unassigned.length} unassigned, ${dead} not reachable`)
for (const f of files) if (!reached.has(f)) console.log(`  not reachable: ${path.relative(renderer, f)}`)
if (unassigned.length) {
  for (const f of unassigned) console.error(`unassigned: ${f}`)
  process.exit(1)
}
