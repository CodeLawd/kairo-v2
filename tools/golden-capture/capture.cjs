const { app, BrowserWindow, protocol, net } = require('electron')
const { pathToFileURL } = require('node:url')
const { createRequire } = require('node:module')
const { mkdtemp, mkdir, realpath, writeFile, rm } = require('node:fs/promises')
const { tmpdir } = require('node:os')
const { basename, dirname, join, relative, resolve, sep } = require('node:path')
const assert = require('node:assert/strict')
const corpus = require('./corpus.cjs')

const width = 1920
const height = 1080
const samples = new Set([
  'broadcast-kjv-original-fit',
  'warm-paper-kjv-original-fit',
  'midnight-kjv-original-fit',
  'lyrics-stage-yoruba-original-fit',
  'lyrics-lower-yoruba-original-fit',
])
protocol.registerSchemesAsPrivileged([{ scheme: 'pa-media', privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true, bypassCSP: true } }])
app.disableHardwareAcceleration()
app.commandLine.appendSwitch('force-device-scale-factor', '1')
app.on('window-all-closed', () => {})

function inspectSlide() {
  function rect(node) {
    const { x, y, width, height } = node.getBoundingClientRect()
    return { x, y, width, height }
  }
  function color(value) {
    const canvas = document.createElement('canvas')
    canvas.width = canvas.height = 1
    const context = canvas.getContext('2d')
    context.fillStyle = value
    context.fillRect(0, 0, 1, 1)
    return Array.from(context.getImageData(0, 0, 1, 1).data)
  }
  const output = []
  for (const kind of ['verse', 'reference']) {
    const box = document.querySelector(`.pa-${kind}-box`)
    if (!box) continue
    const element = box.querySelector(`.pa-${kind}`)
    const text = element.querySelector('.pa-verse-text') || element
    const style = getComputedStyle(element)
    const boxStyle = getComputedStyle(box)
    const padding = parseFloat(boxStyle.paddingLeft)
    const lineMap = new Map()
    const walker = document.createTreeWalker(text, NodeFilter.SHOW_TEXT)
    const segmenter = new Intl.Segmenter('en', { granularity: 'grapheme' })
    const range = document.createRange()
    while (walker.nextNode()) {
      const node = walker.currentNode
      for (const { segment, index } of segmenter.segment(node.data)) {
        range.setStart(node, index)
        range.setEnd(node, index + segment.length)
        const bounds = range.getBoundingClientRect()
        if (bounds.height <= 0 || (bounds.width <= 0 && /\s/u.test(segment))) continue
        const key = Math.round(bounds.top * 2) / 2
        const line = lineMap.get(key) || { text: '', left: Infinity, right: -Infinity, top: Infinity, bottom: -Infinity }
        line.text += segment
        line.left = Math.min(line.left, bounds.left)
        line.right = Math.max(line.right, bounds.right)
        line.top = Math.min(line.top, bounds.top)
        line.bottom = Math.max(line.bottom, bounds.bottom)
        lineMap.set(key, line)
      }
    }
    const lines = [...lineMap.entries()].sort(([a], [b]) => a - b).map(([, line]) => ({
      text: line.text.replace(/[\t\r\n ]+/g, ' ').trim(),
      x: line.left, y: line.top, width: line.right - line.left, height: line.bottom - line.top,
    })).filter(line => line.text)
    output.push({
      kind, box: rect(box), padding, text: text.innerText,
      family: style.fontFamily, weight: Number(style.fontWeight),
      fontSize: parseFloat(style.fontSize), lineHeight: parseFloat(style.lineHeight) / parseFloat(style.fontSize),
      letterSpacing: parseFloat(style.letterSpacing) || 0, align: style.textAlign,
      vertical: boxStyle.justifyContent, decoration: style.textDecorationLine,
      fill: color(style.color), stroke: color(style.webkitTextStrokeColor),
      strokeWidth: parseFloat(style.webkitTextStrokeWidth) || 0,
      autoFit: box.dataset.autoFit === 'true', cap: Number(box.dataset.maxFontPx) || null,
      lines,
    })
  }
  return output
}

async function run(window, source, output) {
  const { build } = createRequire(join(source, 'package.json'))('esbuild')
  const sourceEntry = [
    `export { renderOverlayHTML } from ${JSON.stringify(join(source, 'src/lib/overlay-template.ts'))}`,
    `export { BUILT_IN_THEMES } from ${JSON.stringify(join(source, 'src/lib/theme-library.ts'))}`,
  ].join('\n')
  const bundle = await build({ stdin: { contents: sourceEntry, resolveDir: source, loader: 'ts' },
    bundle: true, write: false, platform: 'node', format: 'cjs' })
  const entry = { exports: {} }
  new Function('module', 'exports', bundle.outputFiles[0].text)(entry, entry.exports)
  const { renderOverlayHTML, BUILT_IN_THEMES } = entry.exports
  const cases = corpus(BUILT_IN_THEMES)
  assert.ok(cases.length >= 300)
  await mkdir(join(output, 'text'), { recursive: true })
  await mkdir(join(output, 'samples'), { recursive: true })
  await window.loadFile(join(source, 'src/main/services/output/overlay.html'))
  const debuggerSession = window.webContents.debugger
  debuggerSession.attach('1.3')
  await debuggerSession.sendCommand('Emulation.setDeviceMetricsOverride', {
    width, height, deviceScaleFactor: 1, mobile: false,
  })
  async function screenshot(path) {
    const { data } = await debuggerSession.sendCommand('Page.captureScreenshot', {
      format: 'png', fromSurface: true, clip: { x: 0, y: 0, width, height, scale: 1 },
    })
    const png = Buffer.from(data, 'base64')
    assert.equal(png.readUInt32BE(16), width)
    assert.equal(png.readUInt32BE(20), height)
    await writeFile(path, png)
  }
  const layouts = []
  for (const item of cases) {
    const html = renderOverlayHTML(item.theme, item.reference, item.text, width, height,
      { coloredLines: item.coloredLines })
    const layers = await window.webContents.executeJavaScript(`(async () => {
      await document.fonts.ready
      window.__setContent(${JSON.stringify(html)}, true)
      return (${inspectSlide.toString()})()
    })()`)
    const expected = Number(Boolean(item.text.trim()))
      + Number(Boolean(item.theme.reference.show && item.reference.trim()))
    assert.equal(layers.length, expected, item.id)
    assert.ok(layers.every(layer => layer.lines.length > 0), item.id)
    for (const layer of layers) {
      layer.source = layer.kind === 'verse' ? item.text : item.reference
      const style = layer.kind === 'verse' ? item.theme.verse : item.theme.reference
      layer.lineHeight = style.lineHeight
      layer.transform = style.textTransform
      layer.shadow = style.shadow
      layer.lineColors = layer.kind === 'verse' ? item.coloredLines?.map(line => line.color || null) ?? [] : []
    }
    layouts.push({ id: item.id, layers })
    if (samples.has(item.id)) {
      await screenshot(join(output, 'samples', item.id + '.png'))
    }
    const css = await window.webContents.insertCSS(
      'html,body{background:#808080!important}.pa-bg{visibility:hidden!important}'
    )
    try {
      await screenshot(join(output, 'text', item.id + '.png'))
    } finally {
      await window.webContents.removeInsertedCSS(css)
    }
  }
  await writeFile(join(output, 'layout.json'), JSON.stringify(layouts) + '\n')
  console.log(`Captured ${layouts.length} cases with Chromium ${process.versions.chrome}`)
}

async function runVisuals(window, source, output, fixtures) {
  const { build } = createRequire(join(source, 'package.json'))('esbuild')
  const entrySource = [
    `export { renderOverlayHTML } from ${JSON.stringify(join(source, 'src/lib/overlay-template.ts'))}`,
    `export { DEFAULT_OVERLAY_THEME, normalizeOverlayTheme } from ${JSON.stringify(join(source, 'src/lib/overlay-defaults.ts'))}`,
    `export { createOverlayElement } from ${JSON.stringify(join(source, 'src/lib/overlay-elements.ts'))}`,
    `export { OVERLAY_SHAPE_GROUPS } from ${JSON.stringify(join(source, 'src/lib/overlay-shapes.ts'))}`,
  ].join('\n')
  const bundle = await build({ stdin: { contents: entrySource, resolveDir: source, loader: 'ts' },
    bundle: true, write: false, platform: 'node', format: 'cjs' })
  const entry = { exports: {} }
  new Function('module', 'exports', bundle.outputFiles[0].text)(entry, entry.exports)
  const cases = require('./visuals.cjs')(entry.exports)
  await mkdir(join(output, 'visuals'), { recursive: true })
  await window.loadFile(join(source, 'src/main/services/output/overlay.html'))
  const session = window.webContents.debugger
  session.attach('1.3')
  await session.sendCommand('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false })
  const gray = await window.webContents.insertCSS('html,body{background:#808080!important}')
  const written = []
  for (const item of cases) {
    const theme = JSON.parse(JSON.stringify(item.theme).replaceAll('@FIX@', fixtures))
    const html = entry.exports.renderOverlayHTML(theme, '', '', width, height)
    await window.webContents.executeJavaScript(`(async () => {
      window.__setContent('', true)
      window.__setContent(${JSON.stringify(html)}, true)
      const media = [...document.querySelectorAll('img, video')]
      await Promise.all(media.map(m => m.decode ? m.decode().catch(() => {}) : null))
      await window.__nextFrame()
    })()`)
    const { data } = await session.sendCommand('Page.captureScreenshot', {
      format: 'png', fromSurface: true, clip: { x: 0, y: 0, width, height, scale: 1 },
    })
    const png = Buffer.from(data, 'base64')
    await writeFile(join(output, 'visuals', item.id + '.png'), png)
    written.push({ id: item.id, group: item.group, theme: item.theme })
  }
  await window.webContents.removeInsertedCSS(gray)
  await writeFile(join(output, 'visuals', 'cases.json'), JSON.stringify(written) + '\n')
  console.log(`Captured ${written.length} visual cases with Chromium ${process.versions.chrome}`)
}

// Prompt 054: program layers, info, stage and test patterns.
async function runPrograms(window, source, output, fixtures) {
  const { build } = createRequire(join(source, 'package.json'))('esbuild')
  const entrySource = [
    `export * from ${JSON.stringify(join(source, 'src/lib/program.ts'))}`,
    `export { outputTestPatternHTML } from ${JSON.stringify(join(source, 'src/lib/output-test-pattern.ts'))}`,
  ].join('\n')
  const bundle = await build({ stdin: { contents: entrySource, resolveDir: source, loader: 'ts' },
    bundle: true, write: false, platform: 'node', format: 'cjs' })
  const entry = { exports: {} }
  new Function('module', 'exports', 'require', bundle.outputFiles[0].text)(entry, entry.exports, createRequire(join(source, 'package.json')))
  const { toDataURL } = createRequire(join(source, 'package.json'))('qrcode')
  // As the audience service makes it.
  const qr = (url) => toDataURL(url, { width: 384, margin: 2, errorCorrectionLevel: 'M' })
  const { cases, clock } = await require('./programs.cjs')(entry.exports, qr, fixtures)
  await mkdir(join(output, 'programs'), { recursive: true })
  // The debugger attaches to a loaded page and stays across loads.
  await window.loadFile(join(source, 'src/main/services/output/overlay.html'))
  const session = window.webContents.debugger
  session.attach('1.3')
  await session.sendCommand('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false })
  const fix = (value) => JSON.parse(JSON.stringify(value).replaceAll('@FIX@', fixtures))
  // `Date` pinned so the clock reads the same in every capture.
  const pin = (now) => `(() => { const Real = Date; const now = ${now};
    class Pinned extends Real { constructor(...a) { super(...(a.length ? a : [now])) } static now() { return now } }
    window.Date = Pinned })()`
  let page = null
  let gray = null
  const written = []
  for (const item of cases) {
    const want = item.stage ? 'stage' : 'overlay'
    if (page !== want) {
      await window.loadFile(join(source, `src/main/services/output/${want}.html`))
      await window.webContents.executeJavaScript(pin(item.now))
      gray = want === 'overlay' ? await window.webContents.insertCSS('html,body{background:#808080!important}') : null
      page = want
    }
    const data = fix(item)
    const script = []
    if (want === 'stage') {
      script.push(`window.__setStage(${JSON.stringify(data.stage)})`)
    } else {
      script.push(`['__setPoll','__setQuestion','__setMessage','__setProps','__setTimerDesign','__setLogo'].forEach(f => window[f](''))`)
      script.push('window.__setInfo(null)')
      script.push('document.getElementById("kairo-diagnostic-pattern")?.remove()')
      const setters = { message: '__setMessage', props: '__setProps', timer: '__setTimerDesign', poll: '__setPoll', question: '__setQuestion', logo: '__setLogo' }
      for (const [kind, { html }] of Object.entries(data.layers || {})) script.push(`window.${setters[kind]}(${JSON.stringify(html)})`)
      if (data.info) script.push(`window.__setInfo(${JSON.stringify(data.info)})`)
      if (data.pattern) script.push(`document.body.insertAdjacentHTML("beforeend", ${JSON.stringify(data.pattern.html)})`)
    }
    await window.webContents.executeJavaScript(`(async () => {
      ${script.join(';\n')}
      const media = [...document.querySelectorAll('img')]
      await Promise.all(media.map(m => m.decode ? m.decode().catch(() => {}) : null))
      await document.fonts.ready
      // Moving parts at their first frame, as a new node starts.
      document.getAnimations().forEach(a => { a.pause(); a.currentTime = 0 })
      await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))
    })()`)
    const { data: png } = await session.sendCommand('Page.captureScreenshot', {
      format: 'png', fromSurface: true, clip: { x: 0, y: 0, width, height, scale: 1 },
    })
    await writeFile(join(output, 'programs', item.id + '.png'), Buffer.from(png, 'base64'))
    const { layers, info, stage, pattern } = item
    written.push({
      id: item.id, group: item.group, now: item.now, clock,
      layers: layers ? Object.fromEntries(Object.entries(layers).map(([k, v]) => [k, v.value])) : undefined,
      info, stage, pattern: pattern ? { pattern: pattern.pattern, name: pattern.name } : undefined,
    })
  }
  if (gray) await window.webContents.removeInsertedCSS(gray)
  await writeFile(join(output, 'programs', 'cases.json'), JSON.stringify(written) + '\n')
  console.log(`Captured ${written.length} program cases with Chromium ${process.versions.chrome}`)
}

async function main() {
  const [, , source, output, mode, fixtures] = process.argv
  if (!source || !output) throw new Error('Usage: electron capture.cjs ELECTRON_CHECKOUT OUTPUT')
  const sourceRoot = await realpath(source)
  const outputRoot = await canonicalDestination(resolve(output))
  const fromSource = relative(sourceRoot, outputRoot)
  if (fromSource === '' || (!fromSource.startsWith('..' + sep) && fromSource !== '..')) {
    throw new Error('Capture output must be outside the Electron source checkout')
  }
  const profile = await mkdtemp(join(tmpdir(), 'kairo-golden-'))
  app.setPath('userData', profile)
  let window
  try {
    await app.whenReady()
    protocol.handle('pa-media', (request) => {
      const path = decodeURIComponent(new URL(request.url).pathname.slice(1))
      return net.fetch(pathToFileURL(path).toString())
    })
    window = new BrowserWindow({
      width, height, useContentSize: true, show: false, transparent: true,
      webPreferences: { backgroundThrottling: false, sandbox: true, contextIsolation: true, // Off-screen, so colours are not converted to the display's profile.
      offscreen: mode === 'visuals' || mode === 'programs' },
    })
    if (mode === 'visuals') await runVisuals(window, sourceRoot, outputRoot, resolve(fixtures))
    else if (mode === 'programs') await runPrograms(window, sourceRoot, outputRoot, resolve(fixtures))
    else await run(window, sourceRoot, outputRoot)
  } finally {
    window?.destroy()
    await rm(profile, { recursive: true, force: true, maxRetries: 3 })
  }
}

async function canonicalDestination(path) {
  try {
    return await realpath(path)
  } catch (error) {
    if (error.code !== 'ENOENT' || dirname(path) === path) throw error
    return join(await canonicalDestination(dirname(path)), basename(path))
  }
}
main().then(() => app.exit(0), error => { console.error(error); app.exit(1) })
