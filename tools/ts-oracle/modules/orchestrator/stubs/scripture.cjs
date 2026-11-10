'use strict'
const log = require('./log.cjs')
// "Book C:V[-E]" or "Book C": verses 1–30 of any chapter exist.
function verses(query) {
  const at = query.lastIndexOf(' ')
  const book = query.slice(0, at)
  const [c, v] = query.slice(at + 1).split(':')
  const chapter = Number(c)
  let range
  if (v === undefined) range = Array.from({ length: 30 }, (_, i) => i + 1)
  else {
    const [s, e] = v.split('-').map(Number)
    range = Array.from({ length: (e ?? s) - s + 1 }, (_, i) => s + i)
  }
  return range.map((verse) => ({ book, chapter, verse, text: `${book} ${chapter}:${verse} text` }))
}
exports.scriptureService = {
  async search(query, translation) { return [{ reference: query, verses: verses(query), translation }] },
  receiveSuggestion(s) { log.cards.push(s.reference) },
  getPendingSuggestion: () => null,
  dismissSuggestion() {},
  requestRowTranslation() {},
  searchLocalQuoteCandidates: () => [],
  searchGeneralQuote: async () => null,
  isGeneralQuoteEligible: () => false,
  onGeneralQuoteCorpusChange: () => () => {},
  getLocalChapterVerses: () => [],
}
