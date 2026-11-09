// Settings defaults as the Electron app builds them, for prompt 012.
//
// `defaults` in src/main/db/index.ts is not exported and that file needs a
// running Electron, so `settingsDefaults` reads the object literal from the
// source with the TypeScript compiler and evaluates it against the real
// exports it names. A change to the literal is picked up without editing this.
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter')
// The Electron checkout's own TypeScript, as the oracle uses its dependencies.
const ts: typeof import('typescript') = createRequire(join(electron, 'package.json'))('typescript')
const dbIndex = join(electron, 'src/main/db/index.ts')

const lib = (name: string) => import(pathToFileURL(join(electron, 'src/lib', `${name}.ts`)).href)

/** What electron-store writes: JSON, so `undefined` properties disappear. */
const onDisk = <T>(value: T): T => JSON.parse(JSON.stringify(value))

export async function settingsDefaults(): Promise<unknown> {
  const source = ts.createSourceFile(dbIndex, readFileSync(dbIndex, 'utf8'), ts.ScriptTarget.Latest, true)
  let literal: ts.Expression | undefined
  const imports = new Map<string, string>()
  for (const statement of source.statements) {
    if (ts.isImportDeclaration(statement) && ts.isStringLiteral(statement.moduleSpecifier)) {
      const from = statement.moduleSpecifier.text
      const named = statement.importClause?.namedBindings
      if (from.startsWith('@shared/') && named && ts.isNamedImports(named)) {
        for (const element of named.elements) imports.set(element.name.text, from.slice('@shared/'.length))
      }
    }
    if (ts.isVariableStatement(statement)) {
      for (const declaration of statement.declarationList.declarations) {
        if (ts.isIdentifier(declaration.name) && declaration.name.text === 'defaults') literal = declaration.initializer
      }
    }
  }
  if (!literal) throw new Error(`no \`const defaults\` in ${dbIndex}`)
  const names: string[] = []
  const values: unknown[] = []
  for (const [name, module] of imports) {
    if (!literal.getText(source).includes(name)) continue
    const mod = await lib(module)
    if (!(name in mod)) throw new Error(`${module} does not export ${name}`)
    names.push(name)
    values.push(mod[name])
  }
  const js = ts.transpile(`(${literal.getText(source)})`, { target: ts.ScriptTarget.ES2022 })
  return onDisk(new Function(...names, `return ${js}`)(...values))
}

/**
 * The settings file a fresh install writes on first launch: the defaults put
 * through the startup sequence of src/main/db/index.ts for an empty profile
 * (no migration flags set). Mirrors that file as of the commit stamped in the
 * oracle cache; prompt 021 ports the sequence itself. Theme ids and times
 * are pinned, so the result is the same on every run.
 */
export async function freshInstall(): Promise<unknown> {
  // Built-in themes get random ids and the current time. Pin both so the
  // cached result only changes when the Electron code does.
  const { randomUUID } = globalThis.crypto
  const now = Date.now
  let next = 0
  globalThis.crypto.randomUUID = () => `00000000-0000-4000-8000-${String(++next).padStart(12, '0')}` as `${string}-${string}-${string}-${string}-${string}`
  Date.now = () => 1_700_000_000_000
  try {
    return await buildFreshInstall()
  } finally {
    globalThis.crypto.randomUUID = randomUUID
    Date.now = now
  }
}

async function buildFreshInstall(): Promise<unknown> {
  const settings = (await settingsDefaults()) as Record<string, any>
  const { retireProPresenterSettings } = await lib('pp-retirement')
  const { normalizeOverlaySettings } = await lib('overlay-defaults')
  const { normalizePresentationSettings } = await lib('program')
  const { assignThemeToOutput, isUntouchedThemeLibrary, normalizeThemeLibrary, seedBuiltInThemes } = await lib('theme-library')
  const { DEFAULT_TRANSLATION_ID } = await lib('bible-translations')

  settings.scripture = { ...settings.scripture, defaultTranslation: DEFAULT_TRANSLATION_ID }
  let store = retireProPresenterSettings(settings) as Record<string, any>
  store = { ...store, overlay: normalizeOverlaySettings(store.overlay) }
  store.presentation = normalizePresentationSettings(store.presentation)
  store.themeLibrary = normalizeThemeLibrary(undefined, store.overlay.theme)
  if (isUntouchedThemeLibrary(store.themeLibrary)) {
    const seeded = seedBuiltInThemes(store.themeLibrary)
    const broadcast = seeded.find((theme: { kind: string }) => theme.kind === 'scripture')
    const stageLyrics = seeded.find((theme: { kind: string }) => theme.kind === 'lyrics')
    const outputs = store.overlay.outputs.map((output: { kind: string }) => {
      if (output.kind !== 'screen') return output
      const withScripture = broadcast ? assignThemeToOutput(output, broadcast) : output
      return stageLyrics ? assignThemeToOutput(withScripture, stageLyrics) : withScripture
    })
    store.themeLibrary = seeded
    store.overlay = normalizeOverlaySettings({ ...store.overlay, outputs })
  }
  return onDisk(store)
}
