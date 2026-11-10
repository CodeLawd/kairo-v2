// Prompt 045's page rendering. Electron rasterises PDFs in the window with
// pdf.js (`Documents.tsx`): scale `min(3840 / w, 2160 / h)`, the canvas
// rounded up, a white background, PNG. This does the same with the app's
// pdfjs-dist under Node, drawing on @napi-rs/canvas (pdf.js's own Node
// canvas), so PDFium's pages can be compared with it.
import { createRequire } from 'node:module'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter')
const require = createRequire(join(electron, 'package.json'))

type Page = { width: number; height: number; png: string }

/** Each PDF is base64; the result is its pages as base64 PNGs. */
export async function render(pdfs: string[]): Promise<Page[][]> {
  const pdfjs = await import(pathToFileURL(join(electron, 'node_modules/pdfjs-dist/legacy/build/pdf.mjs')).href)
  pdfjs.GlobalWorkerOptions.workerSrc = pathToFileURL(join(electron, 'node_modules/pdfjs-dist/legacy/build/pdf.worker.mjs')).href
  const { createCanvas } = require('@napi-rs/canvas')
  const out: Page[][] = []
  for (const data of pdfs) {
    const pdf = await pdfjs.getDocument({ data: new Uint8Array(Buffer.from(data, 'base64')), isEvalSupported: false }).promise
    const pages: Page[] = []
    for (let index = 0; index < pdf.numPages; index++) {
      const page = await pdf.getPage(index + 1)
      const original = page.getViewport({ scale: 1 })
      const viewport = page.getViewport({ scale: Math.min(3840 / original.width, 2160 / original.height) })
      const canvas = createCanvas(Math.ceil(viewport.width), Math.ceil(viewport.height))
      await page.render({ canvasContext: canvas.getContext('2d'), viewport, background: 'white' }).promise
      pages.push({ width: canvas.width, height: canvas.height, png: canvas.toBuffer('image/png').toString('base64') })
      page.cleanup()
    }
    await pdf.destroy()
    out.push(pages)
  }
  return out
}
