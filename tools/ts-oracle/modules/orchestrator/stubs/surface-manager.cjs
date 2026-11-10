'use strict'
const log = require('./log.cjs')
const screen = {
  async showSlide(outputId, reference, text) { log.calls.push(['ShowSlide', outputId, reference, text]); return true },
  async setBackground() { log.calls.push(['SetBackground', 'screen']); return true },
  async swapDocumentPage(outputId, p) { log.calls.push(['SwapDocumentPage', outputId, p]); return false },
  async showDocument(outputId, theme) { log.calls.push(['ShowDocument', outputId, theme.background.mediaPath]); return true },
  async clearText() { log.calls.push(['ClearText', 'screen']); return false },
  async clear() { log.calls.push(['Clear', 'screen']) },
}
exports.surfaceManager = {
  surfaceFor: () => screen,
  outputState: () => ({ reason: 'unavailable' }),
  liveTargets: () => log.outputs.map((output) => ({ output, surface: screen })),
  allSurfaces: () => [screen],
}
