// Calls exports of the Electron app's src/lib. Dev and CI only.
// Input lines:  {"fn":"overlay-boxes.clampOverlayBox","args":[...]}
//               A non-function export with args [] returns its value.
//               "oracle:<module>.<export>" loads tools/ts-oracle/modules/<module>.ts.
// Output lines: {"ok":true,"value":...} or {"ok":false,"error":{...}}
import { createInterface } from 'node:readline'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { dirname, join, resolve } from 'node:path'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter')
// Tests of the runner itself point this at probe modules instead of src/lib.
const lib = resolve(process.env.KAIRO_ORACLE_LIB || join(electron, 'src/lib'))
const output = process.stdout
// The caller closing the pipe is a normal end, not a crash.
output.on('error', e => process.exit(e.code === 'EPIPE' ? 0 : 1))
// Library logging must never corrupt the protocol.
for (const level of ['log', 'info', 'debug']) console[level] = (...a) => console.error(...a)

const tag = (name, extra) => ({ $oracle: name, ...extra })
function encode(v) {
  if (v === undefined) return tag('undefined')
  if (typeof v === 'number' && !Number.isFinite(v)) return tag('number', { value: String(v) })
  if (typeof v === 'bigint' || typeof v === 'function' || typeof v === 'symbol') throw new TypeError(`cannot encode ${typeof v}`)
  if (v === null || typeof v !== 'object') return v
  if (Array.isArray(v)) return v.map(encode)
  if (v instanceof Date) return tag('date', { value: v.toISOString() })
  if (v instanceof Map) return tag('map', { value: [...v].map(([k, x]) => [encode(k), encode(x)]) })
  if (v instanceof Set) return tag('set', { value: [...v].map(encode) })
  if (ArrayBuffer.isView(v)) return tag('bytes', { value: Array.from(v) })
  return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, encode(x)]))
}
function decode(v) {
  if (Array.isArray(v)) return v.map(decode)
  if (v === null || typeof v !== 'object') return v
  if (v.$oracle === 'undefined') return undefined
  if (v.$oracle === 'number') return Number(v.value)
  if (v.$oracle === 'date') return new Date(v.value)
  if (v.$oracle === 'map') return new Map(v.value.map(([k, x]) => [decode(k), decode(x)]))
  if (v.$oracle === 'set') return new Set(v.value.map(decode))
  if (v.$oracle === 'bytes') return Uint8Array.from(v.value)
  return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, decode(x)]))
}

const modules = new Map()
// `oracle:<name>` loads a helper from tools/ts-oracle/modules instead of src/lib.
const helpers = join(dirname(fileURLToPath(import.meta.url)), 'modules')
async function load(name) {
  const helper = name.startsWith('oracle:')
  const path = helper ? name.slice('oracle:'.length) : name
  if (!/^[\w-]+(\/[\w-]+)*$/.test(path)) throw new OracleError(`invalid module name ${name}`)
  if (!modules.has(name)) modules.set(name, await import(pathToFileURL(join(helper ? helpers : lib, path + '.ts')).href))
  return modules.get(name)
}
class OracleError extends Error {}

const fail = (kind, e) => ({ ok: false, error: { kind, name: e?.name ?? 'Error', message: String(e?.message ?? e) } })
async function call(line) {
  let request
  try { request = JSON.parse(line) } catch (e) { return fail('oracle', e) }
  let fn
  try {
    const at = String(request.fn).lastIndexOf('.')
    if (at < 1) throw new OracleError(`expected module.export, got ${request.fn}`)
    if (!Array.isArray(request.args)) throw new OracleError('args must be an array')
    const mod = await load(request.fn.slice(0, at))
    const name = request.fn.slice(at + 1)
    if (!Object.hasOwn(mod, name)) throw new OracleError(`${request.fn} is not exported`)
    fn = mod[name]
    // A constant is read with no arguments, so defaults compare like functions.
    if (typeof fn !== 'function') {
      if (request.args.length) throw new OracleError(`${request.fn} is not an exported function`)
      const value = fn
      fn = () => structuredClone(value)
    }
  } catch (e) { return fail('oracle', e) }
  try {
    let value = fn(...decode(request.args))
    if (value && typeof value.then === 'function') value = await value
    try { return { ok: true, value: encode(value) } } catch (e) { return fail('oracle', e) }
  } catch (e) { return fail('throw', e) }
}

let pending = []
const flush = () => new Promise(done => { if (!pending.length) return done(); const text = pending.join(''); pending = []; output.write(text) ? done() : output.once('drain', done) })
for await (const line of createInterface({ input: process.stdin, crlfDelay: Infinity })) {
  if (!line.trim()) continue
  pending.push(JSON.stringify(await call(line)) + '\n')
  if (pending.length >= 256 || process.stdin.readableLength === 0) await flush()
}
await flush()
