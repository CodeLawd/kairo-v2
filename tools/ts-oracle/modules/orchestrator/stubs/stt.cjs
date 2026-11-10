'use strict'
const finals = new Set(), interims = new Set(), history = []
exports.sttService = {
  onTranscript: (cb) => finals.add(cb), offTranscript: (cb) => finals.delete(cb),
  onInterim: (cb) => interims.add(cb), offInterim: (cb) => interims.delete(cb),
  configure() {}, async start() {}, stop() {},
  getHistory: () => [...history], setHistory(h) { history.splice(0, history.length, ...h) },
  deepgram: { attachStream() {}, detachStream() {} },
  emitFinal(r) { history.push(r); for (const cb of finals) cb(r) },
}
