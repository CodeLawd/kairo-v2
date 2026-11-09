// JavaScript built-ins that `kairo_domain::js` reproduces (prompt 013). Each
// export is the plain built-in, so the Rust helpers are checked against Node.
export const mathRound = (x: number): number => Math.round(x)
export const mathMin = (a: number, b: number): number => Math.min(a, b)
export const mathMax = (a: number, b: number): number => Math.max(a, b)
export const isSafeInteger = (x: number): boolean => Number.isSafeInteger(x)
export const numberToString = (x: number): string => String(x)
export const valueToString = (v: unknown): string => String(v)
export const truthy = (v: unknown): boolean => Boolean(v)
export const stringToNumber = (s: string): number => Number(s)
export const parseIntJs = (s: string, radix?: number): number => parseInt(s, radix)
export const parseFloatJs = (s: string): number => parseFloat(s)
export const trim = (s: string): string => s.trim()
export const replaceAll = (s: string, pattern: string, replacement: string): string =>
  s.replaceAll(pattern, replacement)
