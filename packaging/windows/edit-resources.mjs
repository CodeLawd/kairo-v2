// Give Kairo.exe and kairo-engine.exe the icon and version information that
// electron-builder's rcedit step gave the Electron Kairo.exe (prompt 102).
// Explorer, the taskbar, shortcuts and the uninstall entry show them.
//
//   node edit-resources.mjs <staged-app-dir> <version> <icon.ico>
//
// Uses resedit (pinned in package.json), the library electron-builder itself
// uses for this. Works on any OS: it rewrites the PE resource section only.
import { readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'
import * as ResEdit from 'resedit'

const [dir, version, iconPath] = process.argv.slice(2)
if (!dir || !version || !iconPath) {
  console.error('usage: node edit-resources.mjs <staged-app-dir> <version> <icon.ico>')
  process.exit(2)
}
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(`edit-resources: version must be major.minor.patch, got '${version}'`)
  process.exit(2)
}

const COMPANY = 'Alexander Joshua'
const COPYRIGHT = 'Copyright © 2026 Kairo'
const EN_US = { lang: 0x0409, codepage: 1200 }

const targets = [
  { file: 'Kairo.exe', description: 'Kairo' },
  { file: 'kairo-engine.exe', description: 'Kairo output engine' },
]

const icon = ResEdit.Data.IconFile.from(await readFile(iconPath))
if (icon.icons.length === 0) {
  console.error(`edit-resources: ${iconPath} holds no icons`)
  process.exit(1)
}

for (const { file, description } of targets) {
  const full = path.join(dir, file)
  const exe = ResEdit.NtExecutable.from(await readFile(full))
  const res = ResEdit.NtExecutableResource.from(exe)
  const existing = ResEdit.Resource.VersionInfo.fromEntries(res.entries)
  const vi = existing.length > 0 ? existing[0] : ResEdit.Resource.VersionInfo.createEmpty()
  const [major, minor, patch] = version.split('.').map(Number)
  vi.setFileVersion(major, minor, patch, 0, EN_US.lang)
  vi.setProductVersion(major, minor, patch, 0, EN_US.lang)
  vi.setStringValues(EN_US, {
    CompanyName: COMPANY,
    FileDescription: description,
    FileVersion: version,
    InternalName: path.parse(file).name,
    LegalCopyright: COPYRIGHT,
    OriginalFilename: file,
    ProductName: 'Kairo',
    ProductVersion: version,
  })
  vi.outputToResourceEntries(res.entries)
  // Icon group 1 is the one Windows shows for the file.
  ResEdit.Resource.IconGroupEntry.replaceIconsForResource(
    res.entries,
    1,
    EN_US.lang,
    icon.icons.map((i) => i.data),
  )
  res.outputResource(exe)
  await writeFile(full, Buffer.from(exe.generate()))
  console.log(`edit-resources: ${file} ${version} (${description}), ${icon.icons.length} icon sizes`)
}
