import { test } from 'node:test'
import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'

const electron = path.resolve(process.env.BRIDGE_ELECTRON ?? '../kairo-presenter')
function fixture(t) {
  const dir = mkdtempSync(path.join(tmpdir(), 'bridge-test-'))
  t.after(() => rmSync(dir, { recursive: true, force: true }))
  return dir
}
function run(dir, ...args) {
  return spawnSync(process.execPath, ['packaging/bridge/check-electron-bridge.mjs', '--dir', dir, '--electron', electron, ...args], { encoding: 'utf8' })
}
function windows(dir, { version = '2.0.0', corrupt = false, elevation = false } = {}) {
  const name = `Kairo-Setup-${version}.exe`
  const bytes = Buffer.from('test installer bytes; never executed')
  writeFileSync(path.join(dir, name), bytes)
  writeFileSync(path.join(dir, 'latest.yml'), `version: ${version}\nfiles:\n  - url: ${name}\n    size: ${bytes.length}\n    sha512: ${createHash('sha512').update(corrupt ? 'other bytes' : bytes).digest('base64')}\nisAdminRightsRequired: ${elevation}\n`)
}
test('missing selected feed fails instead of reporting a successful release check', t => {
  const result = run(fixture(t), '--win-only')
  assert.equal(result.status, 1, result.stdout + result.stderr)
  assert.match(result.stdout, /FAIL.*no latest.yml/)
})
test('a partial release fails the default all-platform check', t => {
  const dir = fixture(t); windows(dir)
  const result = run(dir)
  assert.equal(result.status, 1, result.stdout + result.stderr)
})
test('conflicting platform selectors are rejected', t => {
  const dir = fixture(t); windows(dir)
  const result = run(dir, '--win-only', '--linux-only')
  assert.equal(result.status, 2, result.stdout + result.stderr)
})
test('unknown options are rejected', t => {
  const dir = fixture(t); windows(dir)
  assert.equal(run(dir, '--win-only', '--typo', 'value').status, 2)
})
test('valid Windows feed passes its selected checks', t => {
  const dir = fixture(t); windows(dir)
  const result = run(dir, '--win-only')
  assert.equal(result.status, 0, result.stdout + result.stderr)
})
for (const [name, options] of [['hash mismatch', { corrupt: true }], ['downgrade', { version: '1.0.32' }], ['elevation', { elevation: true }]]) {
  test(`Windows rejects ${name}`, t => {
    const dir = fixture(t); windows(dir, options)
    assert.equal(run(dir, '--win-only').status, 1)
  })
}

test('Windows reports missing artifact bytes', t => {
  const dir = fixture(t); windows(dir)
  rmSync(path.join(dir, 'Kairo-Setup-2.0.0.exe'))
  const result = run(dir, '--win-only')
  assert.equal(result.status, 1)
  assert.match(result.stdout, /in the feed but not in/)
})
for (const option of [['--dir'], ['--from', '--win-only'], ['--win-only', '--win-only']]) {
  test(`rejects invalid arguments ${option.join(' ')}`, t => {
    const result = run(fixture(t), ...option)
    assert.equal(result.status, 2, result.stdout + result.stderr)
  })
}
function linux(dir, omit) {
  const version = '2.0.0'
  const names = [`Kairo-${version}.AppImage`, `kairo-presenter_${version}_amd64.deb`]
  const bytes = Buffer.from('test package; never installed')
  for (const name of names) writeFileSync(path.join(dir, name), bytes)
  const entries = names.filter(name => !name.endsWith(omit ?? '.absent')).map(name =>
    `  - url: ${name}\n    size: ${bytes.length}\n    sha512: ${createHash('sha512').update(bytes).digest('base64')}`)
  writeFileSync(path.join(dir, 'latest-linux.yml'), `version: ${version}\nfiles:\n${entries.join('\n')}\n`)
}
test('Linux checks both package types with the real deployed pickers', t => {
  const dir = fixture(t); linux(dir)
  const result = run(dir, '--linux-only')
  assert.equal(result.status, 0, result.stdout + result.stderr)
})
for (const missing of ['.deb', '.AppImage']) {
  test(`Linux refuses a feed missing ${missing}`, t => {
    const dir = fixture(t); linux(dir, missing)
    assert.equal(run(dir, '--linux-only').status, 1)
  })
}
test('malformed YAML cannot pass validation', t => {
  const dir = fixture(t)
  writeFileSync(path.join(dir, 'latest.yml'), 'files: [unterminated')
  const result = run(dir, '--win-only')
  assert.notEqual(result.status, 0)
})

function mac(dir, bundleName = 'Kairo.app') {
  const root = path.join(dir, 'root')
  const app = path.join(root, bundleName)
  mkdirSync(path.join(app, 'Contents', 'MacOS'), { recursive: true })
  copyFileSync('/usr/bin/true', path.join(app, 'Contents', 'MacOS', 'Kairo'))
  writeFileSync(path.join(app, 'Contents', 'Info.plist'), `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.kairo.app</string><key>CFBundleExecutable</key><string>Kairo</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>`)
  execFileSync('codesign', ['--force', '--sign', '-', app], { stdio: 'pipe' })
  const names = ['Kairo-2.0.0-arm64-mac.zip', 'Kairo-2.0.0-mac.zip']
  const entries = names.map(name => {
    const zip = path.join(dir, name)
    execFileSync('ditto', ['-c', '-k', '--keepParent', app, zip])
    const bytes = readFileSync(zip)
    return `  - url: ${name}\n    size: ${bytes.length}\n    sha512: ${createHash('sha512').update(bytes).digest('base64')}`
  })
  writeFileSync(path.join(dir, 'latest-mac.yml'), `version: 2.0.0\nminimumSystemVersion: 22.0.0\nfiles:\n${entries.join('\n')}\n`)
}
test('macOS rejects a differently named app at the zip root', { skip: process.platform !== 'darwin' }, t => {
  const dir = fixture(t); mac(dir, 'Wrong.app')
  const result = run(dir, '--mac-only', '--installed', path.join(dir, 'absent.app'))
  assert.equal(result.status, 1, result.stdout + result.stderr)
  assert.match(result.stdout, /FAIL.*unpacks to one Kairo.app.*Wrong.app/)
})
test('macOS cannot pass without an installed signing reference', { skip: process.platform !== 'darwin' }, t => {
  const dir = fixture(t); mac(dir)
  const result = run(dir, '--mac-only', '--installed', path.join(dir, 'absent.app'))
  assert.equal(result.status, 1, result.stdout + result.stderr)
  assert.match(result.stdout, /0 failed, 1 skipped/)
})

test('Windows rejects elevation on the selected file, where electron-updater reads it', t => {
  const dir = fixture(t); windows(dir)
  const feed = path.join(dir, 'latest.yml')
  writeFileSync(feed, readFileSync(feed, 'utf8').replace('    size:', '    isAdminRightsRequired: true\n    size:'))
  const result = run(dir, '--win-only')
  assert.equal(result.status, 1, result.stdout + result.stderr)
})
