// ESM resolution of the same stubs, for `import ... from 'electron'`.
const stubs = {
  electron: new URL('./electron-stub.cjs', import.meta.url).href,
  'electron-log': new URL('./log-stub.cjs', import.meta.url).href,
  'electron-log/main': new URL('./log-stub.cjs', import.meta.url).href,
}
export async function resolve(specifier, context, next) {
  if (specifier in stubs) return { url: stubs[specifier], shortCircuit: true, format: 'commonjs' }
  return next(specifier, context)
}
