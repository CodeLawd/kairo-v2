#!/usr/bin/env node
// Generates native-migration/api-inventory.md from the Electron TypeScript AST.
// Reads the Electron checkout only. Usage: api-inventory.cjs [ELECTRON_DIR]
const fs = require('node:fs')
const path = require('node:path')
const { createRequire } = require('node:module')

const root = path.resolve(__dirname, '../..')
const electron = fs.realpathSync(path.resolve(process.argv[2] || path.join(root, '../kairo-presenter')))
const outFile = path.join(root, 'native-migration/api-inventory.md')
const ts = createRequire(path.join(electron, 'package.json'))('typescript')
const owners = JSON.parse(fs.readFileSync(path.join(__dirname, 'api-owners.json'), 'utf8'))

function walkDir(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) { if (entry.name !== 'node_modules') walkDir(full, out) } else if (/\.tsx?$/.test(entry.name)) out.push(full)
  }
  return out
}
const rel = file => path.relative(electron, file).split(path.sep).join('/')
const src = path.join(electron, 'src')
const files = walkDir(src)
const program = ts.createProgram(files.filter(f => !f.endsWith('.d.ts') || f.endsWith('env.d.ts')), {
  noEmit: true, skipLibCheck: true, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext,
  moduleResolution: ts.ModuleResolutionKind.Bundler, jsx: ts.JsxEmit.ReactJSX, baseUrl: electron,
  paths: { '@shared/*': ['src/lib/*'], '@renderer/*': ['src/renderer/src/*'] }, strict: true,
})
const checker = program.getTypeChecker()
const sourceFiles = files.map(f => program.getSourceFile(f)).filter(Boolean)
const walk = (node, visit) => { visit(node); ts.forEachChild(node, c => walk(c, visit)) }
const unwrap = n => { while (n && (ts.isAsExpression(n) || ts.isParenthesizedExpression(n) || (ts.isSatisfiesExpression && ts.isSatisfiesExpression(n)))) n = n.expression; return n }
const loc = node => { const sf = node.getSourceFile(); return `${rel(sf.fileName)}:${sf.getLineAndCharacterOfPosition(node.getStart()).line + 1}` }

// ─── Channel constants ───
const constants = new Map(), defined = new Map()
function record(name, init, sf) {
  init = unwrap(init)
  if (ts.isStringLiteralLike(init)) { constants.set(name, init.text); if (rel(sf.fileName).startsWith('src/lib/') && /^[a-z][\w-]*:[\w:.-]+$/.test(init.text)) defined.set(init.text, { name, at: loc(init) }) }
  else if (ts.isObjectLiteralExpression(init)) for (const p of init.properties) if (ts.isPropertyAssignment(p)) record(`${name}.${p.name.getText()}`, p.initializer, sf)
}
for (const sf of sourceFiles) for (const st of sf.statements) if (ts.isVariableStatement(st) && st.declarationList.flags & ts.NodeFlags.Const)
  for (const d of st.declarationList.declarations) if (d.initializer && ts.isIdentifier(d.name) && /^[A-Z][A-Z0-9_]*$/.test(d.name.text)) record(d.name.text, d.initializer, sf)
const unresolved = []
function channelOf(expr, at) {
  expr = unwrap(expr)
  if (ts.isStringLiteralLike(expr)) return expr.text
  const text = expr.getText().replace(/\s+/g, '')
  if (constants.has(text)) return constants.get(text)
  unresolved.push({ text, at: at || loc(expr) })
  return null
}

// ─── Types ───
const flags = ts.TypeFormatFlags.NoTruncation | ts.TypeFormatFlags.UseAliasDefinedOutsideCurrentScope
const clean = s => s.replace(/import\("[^"]*"\)\./g, '').replace(/\s+/g, ' ').replace(/\|/g, '\\|')
function declaredSignature(fn) {
  // Prefer the KairoAPI interface: implementations return untyped ipcRenderer.invoke.
  let holder = fn
  if (ts.isArrowFunction(fn) || ts.isFunctionExpression(fn)) holder = fn.parent
  const object = holder.parent
  if (!object || !ts.isObjectLiteralExpression(object) || !holder.name) return null
  const context = checker.getContextualType(object)
  const property = context && context.getProperty(holder.name.getText())
  if (!property) return null
  return checker.getTypeOfSymbolAtLocation(property, fn).getNonNullableType().getCallSignatures()[0] || null
}
function signatureOf(fn) {
  const sig = declaredSignature(fn) || checker.getSignatureFromDeclaration(fn)
  if (!sig) return { args: '', result: '' }
  const args = sig.getParameters().map(p => `${p.name}: ${checker.typeToString(checker.getTypeOfSymbolAtLocation(p, fn), fn, flags)}`).join(', ')
  return { args: clean(args), result: clean(checker.typeToString(sig.getReturnType(), fn, flags)) }
}

// ─── Preload: what the renderer can call ───
const calls = []
function namespaceMap(sf) {
  const map = new Map()
  walk(sf, n => { if (ts.isVariableDeclaration(n) && n.name.getText() === 'api' && n.initializer) { const o = unwrap(n.initializer); if (ts.isObjectLiteralExpression(o)) for (const p of o.properties) { if (ts.isShorthandPropertyAssignment(p)) map.set(p.name.text, p.name.text); else if (ts.isPropertyAssignment(p) && ts.isIdentifier(unwrap(p.initializer))) map.set(unwrap(p.initializer).text, p.name.getText()) } } })
  return map
}
function owningMethod(node) {
  const chain = []; let fn = null, top = null
  for (let n = node.parent; n; n = n.parent) {
    if (ts.isMethodDeclaration(n) || (ts.isPropertyAssignment(n) && (ts.isArrowFunction(unwrap(n.initializer)) || ts.isFunctionExpression(unwrap(n.initializer))))) { chain.unshift(n.name.getText()); fn ||= ts.isMethodDeclaration(n) ? n : unwrap(n.initializer) }
    else if (ts.isPropertyAssignment(n)) chain.unshift(n.name.getText())
    if (ts.isVariableDeclaration(n)) { top = n.name.getText(); /* outermost wins */ if (!fn && n.initializer) { const i = unwrap(n.initializer); if (ts.isArrowFunction(i)) { fn = i } } }
    if (ts.isFunctionDeclaration(n) && !top) { top = n.name?.getText() }
  }
  return { chain, fn, top }
}
for (const sf of sourceFiles.filter(s => rel(s.fileName).startsWith('src/preload/'))) {
  const ns = namespaceMap(sf)
  const file = path.basename(sf.fileName, '.ts')
  walk(sf, n => {
    if (!ts.isCallExpression(n)) return
    const callee = n.expression.getText().replace(/<.*>$/, '')
    let direction = null, channelArg = n.arguments[0]
    if (['ipcRenderer.invoke', 'ipcRenderer.sendSync'].includes(callee)) direction = 'invoke'
    else if (callee === 'ipcRenderer.send') direction = 'send'
    else if (callee === 'subscribe' || callee === 'ipcRenderer.on' || callee === 'ipcRenderer.once') direction = 'push'
    if (!direction || !channelArg) return
    const { chain, fn, top } = owningMethod(n)
    if (top === 'subscribe') return
    const channel = channelOf(channelArg)
    if (!channel) return
    let namespace, method
    if (file === 'index') {
      if (top === 'api') { namespace = chain[0]; method = chain.slice(1).join('.') || chain[0] }
      else { namespace = ns.get(top) || top; method = chain.join('.') || top }
    } else { namespace = file === 'program' ? 'kairoProgram' : 'audienceSlido'; method = chain.join('.') || 'read' }
    calls.push({ namespace, method, channel, direction, ...(fn ? signatureOf(fn) : { args: '', result: '' }), preload: loc(n), handlers: [], callers: [] })
  })
}

// ─── Main process: handlers and pushes ───
const handlers = new Map(), pushes = new Map(), counts = { viaWrapper: 0, literal: 0, handle: 0, handleInIpcIndex: 0, on: 0, onInIpcIndex: 0 }
const add = (map, ch, at) => { if (!map.has(ch)) map.set(ch, new Set()); map.get(ch).add(at) }
for (const sf of sourceFiles.filter(s => rel(s.fileName).startsWith('src/main/'))) {
  const inIndex = rel(sf.fileName) === 'src/main/ipc/index.ts'
  walk(sf, n => {
    if (!ts.isCallExpression(n) || !n.arguments.length) return
    const callee = n.expression.getText()
    const first = n.arguments[0]
    if (callee === 'handle' && inIndex || callee === 'ipcMain.handle' || callee === 'ipcMain.on' || callee === 'ipcMain.once') {
      if (ts.isIdentifier(unwrap(first)) && first.getText() === 'channel') return // the wrapper's own body
      const on = callee !== 'handle' && callee !== 'ipcMain.handle'
      if (callee === 'handle') counts.viaWrapper++; else if (callee === 'ipcMain.handle') counts.literal++
      counts[on ? 'on' : 'handle']++; if (inIndex) counts[on ? 'onInIpcIndex' : 'handleInIpcIndex']++
      const ch = channelOf(first); if (ch) add(handlers, ch, loc(n))
    } else if (/(^|\.)(send|broadcast\w*|emit\w*|push\w*|notify\w*)$/.test(callee)) {
      const e = unwrap(first)
      const text = e.getText().replace(/\s+/g, '')
      const known = ts.isStringLiteralLike(e) ? (defined.has(e.text) ? e.text : null) : constants.get(text)
      if (known) add(pushes, known, loc(n))
    }
  })
}

// ─── Renderer callers ───
const callers = new Map()
for (const sf of sourceFiles.filter(s => rel(s.fileName).startsWith('src/renderer/'))) walk(sf, n => {
  if (!ts.isPropertyAccessExpression(n)) return
  const m = n.getText().replace(/\s+/g, '').replace(/\?\./g, '.').match(/^(?:window\.)?api\.(\w+)\.(\w+)(?:\.(\w+))?$/)
  if (!m) return
  for (const key of [`${m[1]}.${m[2]}`, m[3] && `${m[1]}.${m[2]}.${m[3]}`].filter(Boolean)) { if (!callers.has(key)) callers.set(key, new Set()); callers.get(key).add(path.basename(sf.fileName)) }
})

// ─── Rows ───
const seen = new Set(), rows = []
for (const c of calls) {
  c.handlers = c.direction === 'push' ? [...(pushes.get(c.channel) || [])].sort() : [...(handlers.get(c.channel) || [])].sort()
  c.callers = c.namespace === 'kairoProgram' ? ['overlay.html'] : c.namespace === 'audienceSlido' ? ['Slido page'] : [...(callers.get(`${c.namespace}.${c.method}`) || [])].sort()
  rows.push(c); seen.add(c.channel)
}
for (const [channel, at] of handlers) if (!seen.has(channel)) rows.push({ namespace: '(main only)', method: defined.get(channel)?.name || channel, channel, direction: 'invoke', args: '', result: '', handlers: [...at].sort(), callers: [] })
for (const [channel, at] of pushes) if (!seen.has(channel) && !handlers.has(channel)) rows.push({ namespace: '(main only)', method: defined.get(channel)?.name || channel, channel, direction: 'push', args: '', result: '', handlers: [...at].sort(), callers: [] })
rows.sort((a, b) => (`${a.namespace}.${a.method}.${a.channel}` < `${b.namespace}.${b.method}.${b.channel}` ? -1 : 1))

const ownerOf = r => owners.channels[r.channel] || owners.methods[`${r.namespace}.${r.method}`] || owners.namespaces[r.namespace] || 'unassigned'
const previous = new Map()
if (fs.existsSync(outFile)) for (const line of fs.readFileSync(outFile, 'utf8').split('\n')) {
  const c = line.split(' | ')
  if (line.startsWith('| ') && c.length >= 10 && !line.startsWith('| Namespace')) previous.set(`${c[0].slice(2)}.${c[1]}.${c[2].replace(/`/g, '')}`, c[c.length - 1].replace(/ \|$/, ''))
}
const statusOf = r => previous.get(`${r.namespace}.${r.method}.${r.channel}`) || 'todo'

if (process.env.API_NAMESPACES) { const c = {}; for (const r of rows) c[r.namespace] = (c[r.namespace] || 0) + 1; console.log(JSON.stringify(c)); console.log('unresolved', JSON.stringify(unresolved.slice(0, 40))); console.log(JSON.stringify(counts)); process.exit(0) }

const used = new Set([...seen, ...handlers.keys(), ...pushes.keys()])
const dead = {
  defined: [...defined].filter(([c]) => !used.has(c)).map(([c, d]) => `\`${c}\` (${d.name}, ${d.at})`).sort(),
  noHandler: rows.filter(r => r.direction !== 'push' && r.direction !== undefined && !r.handlers.length).map(r => `\`${r.channel}\` (${r.namespace}.${r.method})`),
  unreached: [...handlers.keys()].filter(c => !seen.has(c)).map(c => `\`${c}\` (${[...handlers.get(c)].join(', ')})`).sort(),
  noPusher: rows.filter(r => r.direction === 'push' && !r.handlers.length).map(r => `\`${r.channel}\` (${r.namespace}.${r.method})`),
  noCaller: [...new Set(rows.filter(r => r.namespace !== '(main only)' && !r.callers.length).map(r => `${r.namespace}.${r.method}`))],
}
const list = (title, items, note) => `\n### ${title} (${items.length})\n\n${note ? note + '\n\n' : ''}${items.length ? items.map(i => `- ${i}`).join('\n') : 'None.'}\n`
let md = `# API inventory\n\nGenerated by \`tools/ts-oracle/api-inventory.cjs\` from the Electron TypeScript source. Do not hand-edit generated columns. Owners live in \`tools/ts-oracle/api-owners.json\`. Only the \`Native status\` column is edited by hand; regeneration preserves it.\n\n`
md += `## Reconciliation\n\n- \`ipcMain.handle\` registrations: ${counts.handle} (${counts.handleInIpcIndex} in \`src/main/ipc/index.ts\`, ${counts.handle - counts.handleInIpcIndex} elsewhere): ${counts.literal} written as \`ipcMain.handle(CHANNEL\` and ${counts.viaWrapper} through the local \`handle()\` wrapper. The audit's 122 is a text count of \`ipcMain.handle\`, which also matches two mentions inside the wrapper and misses wrapper calls. Distinct invoke handler channels: ${handlers.size}.\n- \`ipcMain.on\` or \`once\` registrations: ${counts.on} (${counts.onInIpcIndex} in \`src/main/ipc/index.ts\`).\n- Inventory rows: ${rows.length}.\n- Errors cross as thrown exceptions, shown through \`cleanIpcError\` in \`src/lib/ipc-error.ts\`, which strips Electron's wrapper. The native call layer needs an equivalent operator-facing message.\n`
if (unresolved.length) md += `\nChannel expressions the generator could not resolve (${unresolved.length}): ${[...new Set(unresolved.map(u => `\`${u.text}\` at ${u.at}`))].sort().join('; ')}\n`
md += `\n## Calls\n\n| Namespace | Method | Channel | Direction | Arguments | Result | Handler or pusher | Preload | Renderer callers | Owner | Native status |\n|---|---|---|---|---|---|---|---|---|---|---|\n`
for (const r of rows) md += `| ${r.namespace} | ${r.method} | \`${r.channel}\` | ${r.direction} | ${r.args || '—'} | ${r.result || '—'} | ${r.handlers.join(', ') || 'none'} | ${r.preload || '—'} | ${r.callers.join(', ') || 'none'} | ${ownerOf(r)} | ${statusOf(r)} |\n`
md += `\n## For review: do not port silently\n`
md += list('Channels defined but never used', dead.defined, 'Constants in `src/lib` shaped like channels with no IPC use. `kairo:web-*` are `postMessage` types, not IPC.')
md += list('Preload calls with no handler', dead.noHandler)
md += list('Handlers no preload reaches', dead.unreached, 'May be reached by another channel string. Check each before porting.')
md += list('Push subscriptions with no detected sender', dead.noPusher, 'Detection covers send, broadcast, emit, push and notify calls with a constant channel. A sender using a variable is listed here too.')
md += list('Exposed methods with no renderer caller', dead.noCaller, 'Detection covers `window.api.x.y` and `api.x.y`. Methods passed by destructuring appear here.')
fs.writeFileSync(outFile, md)
console.log(`rows=${rows.length} handle=${counts.handle} index=${counts.handleInIpcIndex} unassigned=${rows.filter(r => ownerOf(r) === 'unassigned').length} unresolved=${unresolved.length} defined-unused=${dead.defined.length} no-handler=${dead.noHandler.length} unreached=${dead.unreached.length} no-pusher=${dead.noPusher.length} no-caller=${dead.noCaller.length}`)
