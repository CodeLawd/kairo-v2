// Bridge test stand-in for Electron Kairo 1.0.33 (prompt 105). Same Electron
// (33.4.11), same electron-updater (6.8.9), same autoDownload=false /
// autoInstallOnAppQuit=true settings as src/main/services/updater/index.ts.
// It refuses to start without its own profile folder, so it never touches the
// real Kairo profile, and it installs by an ordinary quit (no relaunch).
const { app, autoUpdater: squirrel } = require('electron')
const fs = require('fs')
const userData = process.env.BRIDGE_USERDATA
const updaterPath = process.env.BRIDGE_UPDATER
const logFile = process.env.BRIDGE_LOG
if (!userData || !updaterPath || !logFile) { app.exit(2); return }
app.setPath('userData', userData)
app.setPath('logs', userData)
const line = (...a) => fs.appendFileSync(logFile, `${new Date().toISOString()} ${a.map(String).join(' ')}\n`)
const { autoUpdater } = require(updaterPath)
autoUpdater.autoDownload = false
autoUpdater.autoInstallOnAppQuit = true
autoUpdater.logger = { info: (m) => line('info', m), warn: (m) => line('warn', m), error: (m) => line('error', m), debug: (m) => line('debug', m) }
autoUpdater.on('update-available', (i) => { line('EVENT update-available', i.version); autoUpdater.downloadUpdate().catch((e) => line('EVENT download-failed', e.message)) })
autoUpdater.on('update-not-available', (i) => { line('EVENT update-not-available', i && i.version); setTimeout(() => app.quit(), 500) })
autoUpdater.on('download-progress', (p) => line('EVENT progress', Math.round(p.percent)))
autoUpdater.on('update-downloaded', (i) => line('EVENT update-downloaded', i.version))
autoUpdater.on('error', (e) => { line('EVENT error', e && e.message); setTimeout(() => app.exit(1), 500) })
squirrel.on('update-downloaded', () => { line('EVENT squirrel-ready; quitting so ShipIt installs on quit'); setTimeout(() => app.quit(), 1000) })
app.whenReady().then(() => { line('ready', app.getVersion(), app.getPath('userData')); autoUpdater.checkForUpdates().catch((e) => line('EVENT check-failed', e.message)) })
app.on('window-all-closed', () => {})
