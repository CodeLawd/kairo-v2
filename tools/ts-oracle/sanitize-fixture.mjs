// Turns a real Electron store file into a fixture with no secrets or personal data.
// Dev only. Usage: node tools/ts-oracle/sanitize-fixture.mjs <in.json> <out.json>
//
// Structure, numbers, booleans and nulls are kept. Every string is replaced by
// a placeholder (`s1`, `s2`, …; the same input string always maps to the same
// placeholder, so ids that refer to each other still match), except:
//   - string literals that appear in a TypeScript type in src/lib, which are
//     enum values the types must parse ('dark', 'lower-third', …);
//   - colours (#rgb[a], #rrggbb[aa], rgb()/rgba()), which carry no personal data.
// Object keys are kept, except keys that look like data rather than field
// names (they contain '/', '\', a space or '@'), which are mapped like strings.
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || fileURLToPath(new URL('../../../kairo-presenter', import.meta.url)))
const ts = createRequire(join(electron, 'package.json'))('typescript')

function literalsIn(dir, out) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) literalsIn(path, out)
    else if (name.endsWith('.ts')) {
      const source = ts.createSourceFile(path, readFileSync(path, 'utf8'), ts.ScriptTarget.Latest, true)
      const visit = (node) => {
        if (ts.isLiteralTypeNode(node) && ts.isStringLiteral(node.literal)) out.add(node.literal.text)
        ts.forEachChild(node, visit)
      }
      visit(source)
    }
  }
  return out
}

const allowed = literalsIn(join(electron, 'src/lib'), new Set())
const colour = /^(#[0-9a-f]{3,8}|rgba?\([\d\s.,%]+\))$/i
const dataKey = /[/\\ @]/
const placeholders = new Map()
const placeholder = (text) => {
  if (!placeholders.has(text)) placeholders.set(text, `s${placeholders.size + 1}`)
  return placeholders.get(text)
}
const string = (text) => (allowed.has(text) || colour.test(text) ? text : placeholder(text))

function clean(value) {
  if (typeof value === 'string') return string(value)
  if (Array.isArray(value)) return value.map(clean)
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [dataKey.test(k) ? placeholder(k) : k, clean(v)]))
  }
  return value
}

const [input, output] = process.argv.slice(2)
if (!input || !output) {
  console.error('usage: node tools/ts-oracle/sanitize-fixture.mjs <in.json> <out.json>')
  process.exit(1)
}
writeFileSync(output, JSON.stringify(clean(JSON.parse(readFileSync(input, 'utf8'))), null, '\t') + '\n')
console.error(`${output}: ${placeholders.size} distinct strings replaced`)
