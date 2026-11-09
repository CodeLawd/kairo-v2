// Runs the app's real slide parser and HTML generator on a small slide. Writes only to OUTPUT.
const { createRequire } = require('node:module')
const { join } = require('node:path')
const fs = require('node:fs')
const [, , source, output] = process.argv
const { build } = createRequire(join(source, 'package.json'))('esbuild')
build({ stdin: { contents: `export * from ${JSON.stringify(join(source, 'src/main/services/documents/pptx-slides.ts'))}`, resolveDir: source, loader: 'ts' },
  bundle: true, write: false, platform: 'node', format: 'cjs', external: ['electron'] }).then(r => {
  const m = { exports: {} }
  new Function('module', 'exports', 'require', r.outputFiles[0].text)(m, m.exports, createRequire(join(source, 'package.json')))
  const { parseSlide, slideHtml, slideSize, themeColors } = m.exports
  const ns = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"'
  const pres = `<p:presentation ${ns}><p:sldSz cx="12192000" cy="6858000"/></p:presentation>`
  const shape = (t, y, c) => `<p:sp><p:nvSpPr><p:cNvPr id="2" name="T"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="${y}"/><a:ext cx="9144000" cy="1828800"/></a:xfrm><a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="${c}"/></a:solidFill></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:rPr sz="4800"/><a:t>${t}</a:t></a:r></a:p></p:txBody></p:sp>`
  const slide = t => `<p:sld ${ns}><p:cSld><p:spTree>${shape(t, 914400, 'D0E8FF')}${shape('Grace &amp; peace', 3657600, 'FFE8B0')}</p:spTree></p:cSld></p:sld>`
  const size = slideSize(pres), colors = themeColors('')
  const slides = ['Slide one', 'Slide two'].map(t => parseSlide(slide(t), '', size, colors, { layout: '', master: '', layoutRels: '', masterRels: '' }))
  fs.writeFileSync(output, slideHtml(slides, f => 'file:///missing/' + f, colors, size))
  console.log('height', Math.round(1920 * size.cy / size.cx), 'bytes', fs.statSync(output).size)
}).catch(e => { console.error(e); process.exit(1) })
