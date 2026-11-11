// The program layer corpus of prompt 054: every layer and style option of
// `program.ts`, the info layer, the stage display and the test patterns.
// Each case carries the data the native engine receives (`ProgramLayer`,
// `ConfidenceInfo`, `StageView`) next to the markup Electron draws from the
// same inputs. `@FIX@` stands for the fixtures directory.
//
// Timers are paused in every case so a capture does not depend on when it
// ran; the clock is pinned to `NOW` and its text is stored with the case.
module.exports = async function programs(lib, qr, fixtures) {
  const FIX = '@FIX@'
  // Markup is made with the real path (it is URL-encoded inside).
  const real = (v) => JSON.parse(JSON.stringify(v).replaceAll(FIX, fixtures))
  const NOW = Date.UTC(2026, 10, 11, 15, 7, 30)
  const base = lib.normalizePresentationSettings(lib.DEFAULT_PRESENTATION_SETTINGS)
  const cases = []
  const add = (group, id, data) => cases.push({ id: `${group}-${id}`, group, now: NOW, ...data })
  const layer = (kind, value, html) => ({ layers: { [kind]: { value, html } } })

  // ─── Messages ───────────────────────────────────────────────────────────
  const message = (patch, text) => {
    const style = { ...base.message, ...patch }
    const body = text.trim()
    return layer('message', { kind: 'message', text: body, style }, lib.renderMessageHTML(text, style))
  }
  add('message', 'bottom', message({}, 'Parents of child 42, please come to the nursery.'))
  add('message', 'top', message({ position: 'top' }, 'Cars in the red zone, please move them.'))
  add('message', 'breaks', message({}, 'First line\nSecond line'))
  add('message', 'wraps', message({ fontSizePx: 72 }, 'A long notice that is far too long for one line on the screen and so it has to wrap onto another line.'))
  add('message', 'big-colours', message({ fontSizePx: 120, color: '#ffd166', background: '#1b4f72' }, 'Welcome'))
  add('message', 'small-rgba', message({ fontSizePx: 16, background: 'rgba(200, 30, 30, 0.5)' }, 'Small print'))
  add('message', 'ticker-start', message({ scroll: true }, 'Ticker\nwith two lines'))

  // ─── Props ──────────────────────────────────────────────────────────────
  const timerView = (run, rollover = true) => ({ ...run, rollover, now: NOW })
  const idle = { durationSec: 300, endsAt: null, remainingSec: 300 }
  const pausedPart = { durationSec: 300, endsAt: null, remainingSec: 125 }
  const overrun = { durationSec: 60, endsAt: null, remainingSec: -12 }
  const hours = { durationSec: 7200, endsAt: null, remainingSec: 3723 }
  const props = (list, active, run = idle, rollover = true) => {
    const settings = lib.normalizePresentationSettings({ ...base, props: list })
    const view = timerView(run, rollover)
    const live = settings.props.filter((p) => active.includes(p.id) && (p.background || p.objects.length > 0))
    return layer('props', { kind: 'props', props: live, timer: view }, lib.renderPropsHTML(real(settings.props), active, view))
  }
  const box = (x, y, w, h) => ({ x, y, w, h })
  const text = (id, patch) => ({ id, kind: 'text', ...box(0.1, 0.1, 0.5, 0.2), opacity: 1, text: 'Welcome home', fontSizePx: 72, color: '#ffffff', bold: true, align: 'center', ...patch })
  const one = (id, objects, background = null) => [{ id, name: id, background, objects }]
  add('props', 'text-centre', props(one('p', [text('t', {})]), ['p']))
  add('props', 'text-left-regular', props(one('p', [text('t', { align: 'left', bold: false, color: '#ffd166' })]), ['p']))
  add('props', 'text-right', props(one('p', [text('t', { align: 'right', fontSizePx: 48 })]), ['p']))
  add('props', 'text-lines', props(one('p', [text('t', { text: 'Line one\nLine  two with  spaces\n\nAfter a blank', fontSizePx: 40, ...box(0.1, 0.1, 0.6, 0.5) })]), ['p']))
  add('props', 'text-clipped', props(one('p', [text('t', { text: 'This text is much too large for the small box it is in', fontSizePx: 90, ...box(0.2, 0.3, 0.3, 0.15) })]), ['p']))
  add('props', 'text-opacity', props(one('p', [text('t', { opacity: 0.4 })]), ['p']))
  add('props', 'shapes', props(one('p', [
    { id: 'r', kind: 'shape', ...box(0.05, 0.6, 0.3, 0.3), opacity: 1, shape: 'rectangle', fill: '#315ede', radiusPx: 0 },
    { id: 'rr', kind: 'shape', ...box(0.4, 0.6, 0.25, 0.2), opacity: 0.6, shape: 'rectangle', fill: '#e0a030', radiusPx: 40 },
    { id: 'e', kind: 'shape', ...box(0.7, 0.55, 0.25, 0.35), opacity: 1, shape: 'ellipse', fill: 'rgba(160, 80, 224, 0.8)', radiusPx: 0 },
    { id: 'big', kind: 'shape', ...box(0.05, 0.1, 0.2, 0.1), opacity: 1, shape: 'rectangle', fill: '#40c080', radiusPx: 500 },
  ]), ['p']))
  add('props', 'background', props(one('p', [text('t', {})], 'rgba(0, 0, 0, 0.6)'), ['p']))
  add('props', 'images', props(one('p', [
    { id: 'c', kind: 'image', ...box(0.05, 0.1, 0.4, 0.4), opacity: 1, mediaPath: `${FIX}/card.png`, fit: 'contain' },
    { id: 'v', kind: 'image', ...box(0.55, 0.1, 0.4, 0.2), opacity: 0.7, mediaPath: `${FIX}/card.png`, fit: 'cover' },
    { id: 'a', kind: 'image', ...box(0.55, 0.5, 0.2, 0.3), opacity: 1, mediaPath: `${FIX}/card-alpha.png`, fit: 'contain' },
  ]), ['p']))
  add('props', 'two-stacked', props([
    ...one('a', [{ id: 's', kind: 'shape', ...box(0.1, 0.1, 0.5, 0.5), opacity: 1, shape: 'rectangle', fill: '#ff0000', radiusPx: 0 }]),
    ...one('b', [{ id: 's', kind: 'shape', ...box(0.3, 0.3, 0.5, 0.5), opacity: 1, shape: 'rectangle', fill: '#0000ff', radiusPx: 0 }]),
    ...one('c', [text('t', {})]),
  ], ['b', 'a']))
  const readout = (patch) => ({ id: 'r', kind: 'timer', ...box(0.3, 0.05, 0.4, 0.12), opacity: 1, fontSizePx: 96, color: '#5eead4', overrunColor: '#f87171', bold: true, align: 'center', ...patch })
  add('props', 'timer-idle', props(one('p', [readout({})]), ['p'], idle))
  add('props', 'timer-paused', props(one('p', [readout({ align: 'left', bold: false })]), ['p'], pausedPart))
  add('props', 'timer-overrun', props(one('p', [readout({ align: 'right' })]), ['p'], overrun))
  add('props', 'timer-held-at-zero', props(one('p', [readout({})]), ['p'], overrun, false))
  add('props', 'timer-hours', props(one('p', [readout({ fontSizePx: 140, ...box(0.1, 0.4, 0.8, 0.3) })]), ['p'], hours))

  // ─── Timer design ───────────────────────────────────────────────────────
  const timerLayer = (def, run) => {
    const view = timerView(run, def.overrun)
    const prop = { id: `timer-${def.id}`, name: def.name, background: def.theme.background, objects: def.theme.objects }
    return layer('timer', { kind: 'timer', prop, timer: view }, lib.renderTimerHTML(real(def), view))
  }
  const defaultTimer = base.timers[0]
  add('timer', 'default-idle', timerLayer(defaultTimer, idle))
  add('timer', 'default-overrun', timerLayer(defaultTimer, overrun))
  add('timer', 'designed', timerLayer(lib.normalizePresentationSettings({ ...base, timers: [{
    id: 'walk', name: 'Walk in', kind: 'countdown', durationSec: 600, targetTime: '10:00', overrun: false,
    theme: { background: '#101820', objects: [
      { id: 'band', kind: 'shape', ...box(0, 0.7, 1, 0.3), opacity: 1, shape: 'rectangle', fill: '#1b4f72', radiusPx: 0 },
      text('label', { text: 'Service starts in', fontSizePx: 56, ...box(0.1, 0.72, 0.8, 0.1) }),
      readout({ ...box(0.1, 0.8, 0.8, 0.15), fontSizePx: 110, color: '#ffffff' }),
    ] },
  }] }).timers[0], pausedPart))

  // ─── Poll ───────────────────────────────────────────────────────────────
  const poll = (patch, style = {}) => {
    const view = { id: 'p', title: 'Which song should we sing next?', options: [
      { id: 'a', text: 'Amazing Grace' }, { id: 'b', text: 'How Great Thou Art' }, { id: 'c', text: 'Be Thou My Vision' },
    ], multiple: false, state: 'open', showResults: true, totals: { a: 12, b: 7, c: 3 }, voters: 22, voted: [], ...patch }
    const s = { ...base.question, ...style }
    return layer('poll', { kind: 'poll', poll: view, style: s }, lib.renderPollHTML(view, s))
  }
  add('poll', 'default', poll({}))
  add('poll', 'no-votes', poll({ totals: {}, voters: 0 }))
  add('poll', 'serif-narrow', poll({}, { fontFamily: 'serif', widthPct: 60, background: '#2d1b4f', backgroundOpacity: 50 }))
  add('poll', 'mono-big', poll({ title: '<script> & friends', options: [{ id: 'a', text: 'A long option that has to wrap because the row is narrower than it is' }, { id: 'b', text: 'B' }], totals: { a: 3, b: 1 }, voters: 4 }, { fontFamily: 'mono', fontSizePx: 80, color: '#ffd166' }))

  // ─── Question and join ──────────────────────────────────────────────────
  const question = (q, style = {}) => {
    const s = { ...base.question, ...style }
    return layer('question', { kind: 'question', question: q, style: s }, lib.renderQuestionHTML(q, s))
  }
  const asked = { text: 'How do I join a small group this term?', author: 'Ada' }
  add('question', 'default', question(asked))
  add('question', 'no-author', question(asked, { showAuthor: false, align: 'center' }))
  add('question', 'anonymous-top', question({ text: 'Is there parking on site?', author: '' }, { position: 'top', fontFamily: 'serif' }))
  add('question', 'bottom-mono', question({ text: 'Line one\nline  two', author: null }, { position: 'bottom', fontFamily: 'mono', borderRadiusPx: 0, backgroundOpacity: 60, widthPct: 70 }))
  add('question', 'overflow', question({ text: 'Why? '.repeat(120), author: 'Long' }, { fontSizePx: 100 }))
  const qrDataUrl = await qr('https://kairo.church/j/ABC123')
  const join = { kind: 'join', code: 'ABC123', url: 'kairo.church/j/ABC123', qrDataUrl }
  add('question', 'join', question(join))
  add('question', 'join-plain', question(join, { showJoinUrl: false, joinHeadline: '  ', qrSizePx: 160 }))
  add('question', 'join-no-qr', question({ ...join, qrDataUrl: 'https://evil.example/qr.png' }, { joinHeadline: 'Send us your questions' }))

  // ─── Logo ───────────────────────────────────────────────────────────────
  const logo = (l) => layer('logo', { kind: 'logo', logo: l }, lib.renderLogoHTML(real(l), true))
  add('logo', 'colour', logo({ mediaPath: '', background: '#1b4f72' }))
  add('logo', 'image', logo({ mediaPath: `${FIX}/card.png`, background: '#000000' }))

  // ─── Info ───────────────────────────────────────────────────────────────
  const style = { rollover: true, color: '#5eead4', overrunColor: '#f87171', backdrop: false }
  const info = (clock, stageMessage, position, size, patch = {}) => ({
    info: { clock, stageMessage, layout: { position, size }, sizes: lib.confidenceSizes(size), style: { ...style, ...patch } },
  })
  add('info', 'clock-top', info(true, null, 'top', 'medium'))
  add('info', 'clock-bottom-large', info(true, null, 'bottom', 'large'))
  add('info', 'clock-message-small', info(true, 'Two minutes left', 'top', 'small'))
  add('info', 'message-only-top', info(false, 'Wrap up please', 'top', 'medium'))
  add('info', 'message-only-bottom', info(false, 'Wrap up please, the next service is waiting for the room', 'bottom', 'large'))
  add('info', 'clock-message-bottom', info(true, 'Line one\nline two', 'bottom', 'medium'))
  add('info', 'backdrop', info(true, null, 'top', 'medium', { backdrop: true }))

  // ─── Stage ──────────────────────────────────────────────────────────────
  const stage = (patch) => ({ stage: {
    current: { reference: 'John 3:16', text: 'For God so loved the world, that he gave his only begotten Son,\nthat whosoever believeth in him should not perish.' },
    next: { reference: 'John 3:17', text: 'For God sent not his Son into the world to condemn the world.' },
    stageMessage: null, timer: pausedPart, showNext: true, showClock: true, showTimer: true,
    timerStyle: style, ...patch,
  } })
  add('stage', 'default', stage({}))
  add('stage', 'full', stage({ showNext: false }))
  add('stage', 'no-clock', stage({ showClock: false, timer: overrun }))
  add('stage', 'no-timer', stage({ showTimer: false, timer: idle }))
  add('stage', 'message', stage({ stageMessage: 'Two minutes left' }))
  add('stage', 'empty', stage({ current: null, next: null }))
  add('stage', 'next-reference', stage({ next: { reference: 'Psalm 23', text: '' }, current: { reference: '', text: 'Only words' } }))
  add('stage', 'overflow', stage({ current: { reference: 'Long', text: 'Words '.repeat(80) }, timer: hours }))
  add('stage', 'held-at-zero', stage({ timer: overrun, timerStyle: { ...style, rollover: false, color: '#ffffff', overrunColor: '#ff0000' } }))

  // ─── Test patterns ──────────────────────────────────────────────────────
  for (const pattern of ['bars', 'text', 'motion']) {
    add('pattern', pattern, { pattern: { pattern, name: 'Main screen', html: lib.outputTestPatternHTML(pattern, 'Main screen') } })
  }
  return { cases, clock: lib.formatClock(new Date(NOW)) }
}
