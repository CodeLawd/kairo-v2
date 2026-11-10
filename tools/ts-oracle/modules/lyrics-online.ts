// Prompt 041's comparisons: Electron's online lyric parsing, extraction,
// ranking and gloss harvesting, run in this process over recorded HTML.
// Network calls go to a stubbed axios that serves the fixture for the asked
// URL; `electron` and `electron-log` resolve to the launch stubs.
import Module from 'node:module'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

type Json = any

const launch = join(__dirname, 'electron-launch')
const stubs: Record<string, string> = {
  electron: join(launch, 'electron-stub.cjs'),
  'electron-log': join(launch, 'log-stub.cjs'),
  'electron-log/main': join(launch, 'log-stub.cjs'),
}
const original = (Module as Json)._resolveFilename
;(Module as Json)._resolveFilename = function (request: string, ...rest: Json[]) {
  if (request in stubs) return stubs[request]
  return original.call(this, request, ...rest)
}
process.env.KAIRO_STUB_USER_DATA ??= tmpdir()

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter')
const lyrics = (file: string) => import(pathToFileURL(join(electron, 'src/main/services/lyrics', file)).href)
let loaded: Promise<Json> | null = null
function mods(): Promise<Json> {
  loaded ??= (async () => ({
    // The CommonJS instance the Electron services `require`.
    axios: Module.createRequire(join(electron, 'src/main/services/lyrics/scraper.ts'))('axios'),
    normalize: await lyrics('normalize.ts'),
    scraper: await lyrics('scraper.ts'),
    lrclib: await lyrics('lrclib.ts'),
    web: await lyrics('web-lyrics.ts'),
    resolver: await lyrics('snippet-resolver.ts'),
    online: await lyrics('online.ts'),
    glosses: await lyrics('online-glosses.ts'),
  }))()
  return loaded
}

/** Serve `pages[url]` (exact, or by URL prefix before the query) and 404 the rest. */
async function withPages<T>(pages: Record<string, Json>, run: () => Promise<T>): Promise<T> {
  const { axios } = await mods()
  const get = axios.get
  axios.get = async (url: string) => {
    const body = pages[url] ?? pages[url.split('?')[0]]
    if (body === undefined) throw Object.assign(new Error('Request failed with status code 404'), { isAxiosError: true, response: { status: 404 } })
    return { status: 200, data: body }
  }
  try {
    return await run()
  } finally {
    axios.get = get
  }
}

const call = (module: string, fn: string) => async (...args: Json[]) => (await mods())[module][fn](...args)

export const canonicalText = call('normalize', 'canonicalText')
export const structureLyrics = call('normalize', 'structureLyrics')
export const normalizeSectionHeader = call('normalize', 'normalizeSectionHeader')
export const dedupeSections = call('normalize', 'dedupeSections')
export const extractLyricLines = call('normalize', 'extractLyricLines')
export const cleanTrackTitle = call('lrclib', 'cleanTrackTitle')
export const normalizeGeniusLyrics = call('scraper', 'normalizeGeniusLyrics')
export const selectPrimaryLyricSegment = call('scraper', 'selectPrimaryLyricSegment')
export const structureBilingualHymn = call('web', 'structureBilingualHymn')
export const cleanScrapedLyrics = call('web', 'cleanScrapedLyrics')
export const parsePageHeading = call('web', 'parsePageHeading')
export const isAllowedLyricsUrl = call('web', 'isAllowedLyricsUrl')
export const parseDuckHtml = call('resolver', 'parseDuckHtml')
export const parseBraveHtml = call('resolver', 'parseBraveHtml')
export const parseMojeekHtml = call('resolver', 'parseMojeekHtml')
export const looksLikeChallenge = call('resolver', 'looksLikeChallenge')
export const cleanPageTitle = call('resolver', 'cleanPageTitle')
export const discoverSongPages = call('resolver', 'discoverSongPages')
export const coreTitle = call('online', 'coreTitle')
export const needsWebResolution = call('online', 'needsWebResolution')
export const findLyricMatch = call('online', 'findLyricMatch')
export const rankResults = call('online', 'rankResults')

/** The full Genius page path over recorded HTML; errors as `{ error }`. */
export async function geniusFromHtml(url: string, title: string | null, html: string): Promise<Json> {
  const { scraper } = await mods()
  scraper.lyricsScraperService.clearCache()
  try {
    return await withPages({ [url]: html }, () => scraper.lyricsScraperService.fetchLyrics(url, title ? { title } : {}))
  } catch (error) {
    return { error: (error as Error).message }
  }
}

export async function scrapeFromHtml(url: string, html: string): Promise<Json> {
  const { web } = await mods()
  try {
    return await withPages({ [url]: html }, () => web.scrapeLyricsPage(url))
  } catch (error) {
    return { error: (error as Error).message }
  }
}

export async function lrclibSearch(query: string, body: string): Promise<Json> {
  const { lrclib } = await mods()
  lrclib.lrclibService.clearCache()
  return withPages({ 'https://lrclib.net/api/search': JSON.parse(body) }, () => lrclib.lrclibService.search(query))
}

/** Published glosses harvested from the given pages; managed search unavailable. */
export async function glossMap(title: string, artist: string | null, pages: Record<string, string>): Promise<Json> {
  const { glosses } = await mods()
  const map = await withPages(pages, () => glosses.fetchOnlineGlossMap({ title, artist: artist ?? undefined }))
  return [...map.entries()]
}
