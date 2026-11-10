'use strict'
const log = require('./log.cjs')
let live = null, since = 0, revision = 0
exports.programService = {
  getScripture: () => live,
  getScriptureSince: () => since,
  setScriptureSlide(content, meta) {
    log.calls.push(['Program scripture', content.reference])
    live = { book: meta.book, chapter: meta.chapter, verseStart: meta.verseStart, verseEnd: meta.verseEnd, translation: meta.translation, revision: ++revision, passageId: meta.passageId, passageIndex: meta.passageIndex }
    since = 1e15
  },
  setSlide(content, next) { log.calls.push(['Program slide', content.reference, next ? next.reference : '']) },
  clearCurrent() { log.calls.push(['Program clearCurrent']); live = null },
  clearProgram() { log.calls.push(['Program clear']); live = null },
  reset() { live = null; since = 0 },
}
