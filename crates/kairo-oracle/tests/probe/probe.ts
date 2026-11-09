// Exports that exercise every path of tools/ts-oracle/runner.mjs.
export const notAFunction = 1
export const echo = (...args: unknown[]): unknown[] => args
export const nothing = (): undefined => undefined
export const nil = (): null => null
export const withUndefinedField = () => ({ a: 1, b: undefined })
export const sparse = () => [1, undefined, 3]
export const nonFinite = () => [NaN, Infinity, -Infinity]
export const negativeZero = () => -0
export const fails = (): never => { throw new RangeError('out of range') }
export const failsWithString = (): never => { throw 'plain' }
export const rejects = async (): Promise<never> => { throw new TypeError('async') }
export const resolves = async (x: number) => x * 2
export const noisy = (x: number) => { console.log('library noise'); return x }
export const big = () => 10n
export const circular = () => { const o: Record<string, unknown> = {}; o.self = o; return o }
export const collections = () => ({
  when: new Date(Date.UTC(2026, 0, 2)),
  map: new Map([['k', 1]]),
  set: new Set([1, 2]),
  bytes: new Uint8Array([1, 255]),
})
export const typeOf = (x: unknown) => (x instanceof Map ? 'map' : x instanceof Set ? 'set' : x instanceof Date ? 'date' : x instanceof Uint8Array ? 'bytes' : typeof x)
export const defaulted = (a: number, b = 5) => a + b
