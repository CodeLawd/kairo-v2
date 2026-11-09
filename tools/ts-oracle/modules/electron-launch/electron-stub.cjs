// A stand-in for the `electron` module, enough for the Electron app's stores
// and db/index.ts to run under plain Node. userData is KAIRO_STUB_USER_DATA.
'use strict'
const path = require('node:path')
const userData = process.env.KAIRO_STUB_USER_DATA
const paths = { userData, appData: path.dirname(userData), logs: path.join(userData, 'logs'), temp: require('node:os').tmpdir() }
const noop = () => {}
module.exports = {
  app: {
    isPackaged: true,
    getPath: (name) => {
      if (!(name in paths)) throw new Error(`stub: no path ${name}`)
      return paths[name]
    },
    setPath: (name, value) => { paths[name] = value },
    getName: () => 'Kairo',
    getVersion: () => process.env.KAIRO_STUB_VERSION || '1.0.40',
    on: noop,
  },
  ipcMain: { on: noop, handle: noop },
  shell: {},
}
