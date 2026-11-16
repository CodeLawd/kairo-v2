#!/usr/bin/env node
// Checks that a native release would be accepted by the Electron updaters
// already installed in the field (prompt 105).
//
//   node packaging/bridge/check-electron-bridge.mjs --dir <release dir> \
//     --electron <path to kairo-presenter> [--from 1.0.33] \
//     [--installed /Applications/Kairo.app] [--mac-only|--win-only|--linux-only]
//
// It runs electron-updater's own code from kairo-presenter's node_modules
// (the version deployed clients ship, 6.8.9 at the time of writing) and
// kairo-presenter's own `mac-self-update.ts`, read only. Nothing is installed
// or launched, and no file outside a temporary folder is written.
//
// For each platform whose feed is in --dir it checks what the deployed client
// checks before it installs:
//
//   macOS (Squirrel.Mac, Developer ID builds; and the self-update path that
//   ad-hoc signed builds use):
//     - the feed parses with electron-updater's parser and the version is
//       newer than --from;
//     - minimumSystemVersion hides it from macOS 11 and 12 (Darwin 20, 21)
//       and offers it on 13 and later;
//     - MacUpdater.filterFilesForArch + findFile pick the arm64 zip on Apple
//       silicon (and under Rosetta) and the Intel zip on Intel;
//     - the self-update path's pickMacZip picks the same files, and
//       releaseAssetUrl accepts their names;
//     - each zip's SHA-512 and size match the feed;
//     - each zip unpacks with `ditto -x -k` to exactly one .app whose
//       CFBundleIdentifier is the installed app's;
//     - `codesign --verify --deep --strict` passes (self-update path), and
//       the bundle satisfies the installed app's designated requirement
//       (Squirrel.Mac's check, which refuses any other signer).
//   Windows (NsisUpdater): the feed parses, findFile picks the .exe, its
//     SHA-512 and size match, and the feed does not ask for elevation.
//   Linux (AppImageUpdater, DebUpdater): findFile picks the AppImage and the
//     deb, hashes and sizes match, and the deb keeps Electron's file name
//     pattern (`kairo-presenter_<v>_amd64.deb`).
//
// Exit status 0 only when every required check passed; 1 for failures or
// skipped checks. A missing selected feed fails. Use a platform selector for
// a partial check; default validation requires all three feeds and macOS tools.

import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { createReadStream, existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const args = parseArgs(process.argv.slice(2))
const dir = path.resolve(need(args, 'dir'))
const electron = path.resolve(need(args, 'electron'))
const from = args.from ?? '1.0.33'
const installed = args.installed ?? (process.platform === 'darwin' ? '/Applications/Kairo.app' : undefined)
const only = args['mac-only'] ? 'mac' : args['win-only'] ? 'win' : args['linux-only'] ? 'linux' : null

const updaterOut = path.join(electron, 'node_modules', 'electron-updater', 'out')
if (!existsSync(updaterOut)) fail(`electron-updater not found under ${electron}/node_modules (run its install first)`)
const req = createRequire(path.join(updaterOut, 'main.js'))
const os = req('os')
const semver = req('semver')
const updaterVersion = JSON.parse(readFileSync(path.join(updaterOut, '..', 'package.json'), 'utf8')).version
const { parseUpdateInfo, findFile } = req('./providers/Provider')
const { GitHubProvider } = req('./providers/GitHubProvider')
const { AppUpdater } = req('./AppUpdater')
const { MacUpdater } = req('./MacUpdater')
const selfUpdate = !only || only === 'mac'
  ? await import(pathToFileURL(path.join(electron, 'src/main/services/updater/mac-self-update.ts')).href)
  : null

const results = []
const record = (status, name, detail = '') => {
  results.push({ status, name, detail })
  console.log(`${status.padEnd(4)} ${name}${detail ? ` — ${detail}` : ''}`)
}
const check = async (name, fn) => {
  try {
    const detail = await fn()
    record('ok', name, detail ?? '')
  } catch (error) {
    record('FAIL', name, error instanceof Error ? error.message : String(error))
  }
}

console.log(`electron-updater ${updaterVersion}, upgrading from ${from}, release dir ${dir}`)

// The deployed clients read the GitHub provider of package.json `build.publish`.
const pkg = JSON.parse(readFileSync(path.join(electron, 'package.json'), 'utf8'))
const publish = [pkg.build?.publish].flat().find((p) => p?.provider === 'github')
if (!publish) fail('kairo-presenter package.json has no GitHub publish entry')

function provider(platform) {
  return new GitHubProvider(
    { provider: 'github', owner: publish.owner, repo: publish.repo },
    { channel: null, allowPrerelease: false, currentVersion: semver.parse(from) },
    { platform, executor: null, isUseMultipleRangeRequest: true },
  )
}

function readFeed(name) {
  const file = path.join(dir, name)
  if (!existsSync(file)) return null
  const info = parseUpdateInfo(readFileSync(file, 'utf8'), name, new URL(`file://${file}`))
  info.tag = `v${info.version}`
  return info
}

function isOffered(info, darwinRelease) {
  const logger = { info() {}, warn() {}, error() {} }
  const realRelease = os.release
  if (darwinRelease) os.release = () => darwinRelease
  try {
    return AppUpdater.prototype.isUpdateAvailable.call(
      {
        currentVersion: semver.parse(from),
        allowDowngrade: false,
        isUpdateSupported: (i) => AppUpdater.prototype.checkIfUpdateSupported.call({ _logger: logger }, i),
        isUserWithinRollout: () => true,
      },
      info,
    )
  } finally {
    os.release = realRelease
  }
}

async function sha512(file) {
  const hash = createHash('sha512')
  for await (const chunk of createReadStream(file)) hash.update(chunk)
  return hash.digest('base64')
}

async function matchesFeed(entry) {
  const name = path.basename(decodeURIComponent(entry.url.pathname))
  const file = path.join(dir, name)
  if (!existsSync(file)) throw new Error(`${name} is in the feed but not in ${dir}`)
  const size = statSync(file).size
  if (entry.info.size != null && entry.info.size !== size) throw new Error(`${name}: feed size ${entry.info.size}, file ${size}`)
  const digest = await sha512(file)
  if (digest !== entry.info.sha512) throw new Error(`${name}: SHA-512 differs from the feed`)
  return `${name}, ${size} bytes`
}

function run(cmd, argv) {
  return execFileSync(cmd, argv, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
}

function designatedRequirement(bundle) {
  const out = execFileSync('codesign', ['-d', '-r-', bundle], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
  const line = out.split('\n').find((l) => l.startsWith('designated => '))
  if (!line) throw new Error(`no designated requirement on ${bundle}`)
  return line.slice('designated => '.length).trim()
}

function bundleId(bundle) {
  return run('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleIdentifier', path.join(bundle, 'Contents', 'Info.plist')]).trim()
}

async function checkMac() {
  const info = readFeed('latest-mac.yml')
  if (!info) return record('FAIL', 'macOS', 'no latest-mac.yml')
  await check('macOS feed is newer than the installed version', () => {
    if (!semver.gt(info.version, from)) throw new Error(`${info.version} is not above ${from}`)
    return info.version
  })
  for (const [label, darwin, expected] of [
    ['macOS 11 (Darwin 20.6.0)', '20.6.0', false],
    ['macOS 12 (Darwin 21.6.0)', '21.6.0', false],
    ['macOS 13 (Darwin 22.0.0)', '22.0.0', true],
    ['macOS 26 (Darwin 25.0.0)', '25.0.0', true],
  ]) {
    await check(`macOS feed ${expected ? 'offered on' : 'hidden from'} ${label}`, async () => {
      const offered = await isOffered(info, darwin)
      if (offered !== expected) throw new Error(`offered=${offered}; minimumSystemVersion=${info.minimumSystemVersion ?? 'absent'}`)
    })
  }

  const files = provider('darwin').resolveFiles(info)
  const picks = []
  for (const [label, isArm64Mac, nodeArch] of [
    ['Apple silicon', true, 'arm64'],
    ['Intel', false, 'x64'],
  ]) {
    await check(`Squirrel path picks the ${label} zip`, () => {
      const pick = findFile(MacUpdater.filterFilesForArch(files, isArm64Mac), 'zip', ['pkg', 'dmg'])
      if (!pick) throw new Error('ERR_UPDATER_ZIP_FILE_NOT_FOUND: ZIP file not provided')
      const name = path.basename(pick.url.pathname)
      if (/arm64/.test(name) !== isArm64Mac) throw new Error(`picked ${name}`)
      const base = `https://github.com/${publish.owner}/${publish.repo}/releases/download/v${info.version}/`
      if (!pick.url.href.startsWith(base)) throw new Error(`download URL ${pick.url.href}`)
      picks.push(pick)
      return name
    })
    await check(`self-update path picks the ${label} zip`, () => {
      const pick = selfUpdate.pickMacZip(info.files, nodeArch)
      if (!pick) throw new Error('This release has no update file for this Mac.')
      const url = selfUpdate.releaseAssetUrl(info.version, pick.url)
      if (/arm64/.test(pick.url) !== isArm64Mac) throw new Error(`picked ${pick.url}`)
      return url
    })
  }
  await check('Rosetta (x64 Electron on Apple silicon) gets the arm64 zip through Squirrel', () => {
    const pick = findFile(MacUpdater.filterFilesForArch(files, true), 'zip', ['pkg', 'dmg'])
    if (!pick || !/arm64/.test(pick.url.pathname)) throw new Error('arm64 zip not chosen')
  })

  for (const pick of picks) await check(`${path.basename(pick.url.pathname)} matches the feed`, () => matchesFeed(pick))

  if (process.platform !== 'darwin') return record('skip', 'macOS bundle checks', 'need macOS (ditto, codesign)')
  let requirement = null
  let expectedId = null
  if (installed && existsSync(installed)) {
    requirement = designatedRequirement(installed)
    expectedId = bundleId(installed)
    record('ok', 'installed app read', `${installed}: ${expectedId}; ${requirement}`)
  } else {
    record('skip', 'designated requirement', 'no installed Electron app to read it from (--installed)')
  }

  const seen = new Set()
  for (const pick of picks) {
    const name = path.basename(pick.url.pathname)
    if (seen.has(name)) continue
    seen.add(name)
    const work = mkdtempSync(path.join(tmpdir(), 'kairo-bridge-'))
    try {
      const unpacked = path.join(work, 'app')
      await check(`${name} unpacks to one Kairo.app`, () => {
        run('ditto', ['-x', '-k', path.join(dir, name), unpacked])
        const entries = readdirSync(unpacked).filter((e) => !e.startsWith('.'))
        const apps = entries.filter((e) => e.endsWith('.app'))
        if (apps.length !== 1 || entries.length !== 1 || apps[0] !== 'Kairo.app') throw new Error(`zip root holds ${JSON.stringify(entries)}`)
        return apps[0]
      })
      const app = path.join(unpacked, 'Kairo.app')
      if (!existsSync(app)) continue
      await check(`${name}: bundle id matches the installed app`, () => {
        const id = bundleId(app)
        if (id !== 'com.kairo.app' || (expectedId && id !== expectedId)) throw new Error(`The update is not Kairo (${id}).`)
        return id
      })
      await check(`${name}: codesign --verify --deep --strict`, () => {
        run('codesign', ['--verify', '--deep', '--strict', app])
      })
      if (requirement) {
        await check(`${name}: satisfies the installed app's designated requirement (Squirrel.Mac)`, () => {
          run('codesign', ['--verify', '--deep', '--strict', `-R=${requirement}`, app])
        })
      }
    } finally {
      rmSync(work, { recursive: true, force: true })
    }
  }
}

async function checkWindows() {
  const info = readFeed('latest.yml')
  if (!info) return record('FAIL', 'Windows', 'no latest.yml')
  await check('Windows feed is newer than the installed version', async () => {
    if (!(await isOffered(info))) throw new Error(`${info.version} not offered over ${from}`)
    return info.version
  })
  const files = provider('win32').resolveFiles(info)
  const pick = findFile(files, 'exe')
  await check('NsisUpdater picks the installer', () => {
    if (!pick) throw new Error('no .exe in latest.yml')
    const name = path.basename(pick.url.pathname)
    if (name !== `Kairo-Setup-${info.version}.exe`) throw new Error(`picked ${name}`)
    return name
  })
  if (pick) await check('installer matches the feed', () => matchesFeed(pick))
  await check('feed does not require elevation (per-user install, as Electron)', () => {
    if (info.isAdminRightsRequired || pick?.info.isAdminRightsRequired) throw new Error('isAdminRightsRequired is set: electron-updater would run elevate.exe')
  })
}

async function checkLinux() {
  const info = readFeed('latest-linux.yml')
  if (!info) return record('FAIL', 'Linux', 'no latest-linux.yml')
  await check('Linux feed is newer than the installed version', async () => {
    if (!(await isOffered(info))) throw new Error(`${info.version} not offered over ${from}`)
    return info.version
  })
  const files = provider('linux').resolveFiles(info)
  const appImage = findFile(files, 'AppImage', ['rpm', 'deb', 'pacman'])
  const deb = findFile(files, 'deb', ['AppImage', 'rpm', 'pacman'])
  await check('AppImageUpdater picks the AppImage', () => {
    if (!appImage || !appImage.url.pathname.endsWith('.AppImage')) throw new Error('no AppImage in latest-linux.yml')
    return path.basename(appImage.url.pathname)
  })
  await check('DebUpdater picks the deb', () => {
    if (!deb || !deb.url.pathname.endsWith('.deb')) throw new Error('no deb in latest-linux.yml')
    const name = path.basename(deb.url.pathname)
    if (name !== `kairo-presenter_${info.version}_amd64.deb`) throw new Error(`picked ${name}`)
    return name
  })
  if (appImage) await check('AppImage matches the feed', () => matchesFeed(appImage))
  if (deb) await check('deb matches the feed', () => matchesFeed(deb))
}

if (!only || only === 'mac') await checkMac()
if (!only || only === 'win') await checkWindows()
if (!only || only === 'linux') await checkLinux()

const failed = results.filter((r) => r.status === 'FAIL').length
const skipped = results.filter((r) => r.status === 'skip').length
console.log(`\n${results.length - failed - skipped} passed, ${failed} failed, ${skipped} skipped`)
process.exit(failed || skipped ? 1 : 0)

function parseArgs(argv) {
  const out = {}
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (!a.startsWith('--')) fail(`unexpected argument ${a}`)
    const key = a.slice(2)
    if (!['dir', 'electron', 'from', 'installed', 'mac-only', 'win-only', 'linux-only'].includes(key)) fail(`unknown option ${a}`)
    if (key in out) fail(`duplicate option ${a}`)
    if (['mac-only', 'win-only', 'linux-only'].includes(key)) out[key] = true
    else {
      const value = argv[++i]
      if (!value || value.startsWith('--')) fail(`${a} needs a value`)
      out[key] = value
    }
  }
  if (['mac-only', 'win-only', 'linux-only'].filter((key) => out[key]).length > 1) fail('choose only one platform selector')
  return out
}

function need(opts, key) {
  return opts[key] ?? fail(`--${key} is required`)
}

function fail(message) {
  console.error(`check-electron-bridge: ${message}`)
  process.exit(2)
}
