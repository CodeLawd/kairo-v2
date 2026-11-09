// Runs the Electron app's real settings startup (src/main/db/index.ts) in
// this process against KAIRO_STUB_USER_DATA, then exits.
const electron = process.env.KAIRO_ELECTRON_DIR!
await import(`${electron}/src/main/db/index.ts`)
