# 075 source error, empty and failure paths

Read: every file in Electron `components/screens/`, `capture/CaptureSettings.tsx`, `screen-configuration.integration.cjs`, plus imported display/theme/output rules, SetupChecklist and TransitionPanel. The only committed drift in this source area since `de3bb54` was `af978c5`; the listed files had no uncommitted changes when checked.

| Source condition | Native handling |
|---|---|
| No audience outputs; explicitly empty output list | Empty sidebar; Add; preserved by storage and round-trip tests |
| No stage displays | Empty Stage group and Add stage display |
| Nothing selected, unknown selection, missing `kind:` | Select/add guidance; no invented output |
| `add:` mount repeated / invalid kind | Creation only on first successful snapshot; unknown kind ignored |
| Last audience output deleted | Refused with source error; stages may all be removed |
| Right-click on an unselected row | Stable per-row context menu, confirmation targets that row |
| Delete cancelled / target already removed | Cancel changes nothing; missing audience target reports error |
| Output/stage persistence failure | Inline error, saved snapshot retained, rejected numeric values restored |
| Concurrent deferred theme edit | Backend reads latest settings under SettingsWiring lock, patches only its own fields |
| Settings/status read fails | Inline error and Retry for initial load; existing snapshot retained on later failure |
| Initial fetch resolves after a topic push | Push revision suppresses stale initial result |
| No displays / display read failure | Looking for displays; read failure also shown inline |
| Bound monitor unplugged / saved id changed | Saved choice retained and marked not connected; domain resolver rematches by label and size |
| Display occupied by enabled screen/stage | Choice disabled; canvas assignment and backend also reject with owner name |
| Kairo controls on chosen monitor | Rehearsal-window explanation retained |
| No chosen display for frame test | Frame button disabled; changing aspect makes no test request |
| Custom ratio typing | Clamp rounded 1–100; 500 ms debounced preview, dropped with view |
| Unknown/invalid display dimensions | No misleading fit hint; arrangement clamps degenerate bounds |
| No media playlists / deleted chosen playlist / library read failure | Make-a-playlist hint, disabled Deleted playlist choice, error on failed snapshot load |
| Lobby content | Service-only Looks cells unavailable; image hold clamped 3–600 seconds; videos play to end |
| No card provider / driver unavailable | Specific provider/driver reason; no generic driver update advice for profile/empty-card conditions |
| Missing / unsupported Desktop Video | Driver message and official download action |
| Compatibility driver / installed version | Muted compatibility notice and detected version |
| Unavailable card channel | Disabled with its own profile/compatibility reason |
| Saved card unplugged / saved mode unavailable | Saved choice remains visible, marked disconnected/unavailable |
| Port changed | First mode selected, key/fill reset; screen-to-card conversion gets new id and retains content/themes/looks |
| Refresh repeated / request fails / inventory unchanged / no ports | Busy icon, repeat clicks disabled; inline result/error including unchanged and empty inventory |
| Card not ready / test pattern request fails or false | Pattern choices disabled until ready; actionable error; busy released on completion |
| Card diagnostics missing / late/dropped frames | Waiting for the card; real counters and smooth/late/dropped summary |
| Disabled output / no status / not ready / non-Live reason | Off / Checking… / actual reason; lobby readiness reports its own playlist |
| NDI runtime lacks audio / audio query fails | Audio disabled with NDI 6 explanation; no false checked state |
| NDI/Syphon output limit | Model rejects excess outputs; menu offers only available additions |
| No themes / custom theme / lyrics inherit scripture | Custom and Same as scripture choices; missing theme assignment fails explicitly |
| Test verse / Clear false / rejected command | Source failure text or concrete error, busy released; test buttons disabled while pending |
| A topic arrives during name or numeric editing | Focused drafts retained; clean fields reconcile; regression tests |
| Window closes during async requests | Weak view updates ignored; subscriber task aborted by lifetime guard |

CaptureSettings is a related reference, not the Screens window: source selection includes disabled outputs and tells the operator to turn them on in Screens; removed sources fall back through captureSourceFor. Capture settings, folder/audio-input forms and recording lifecycle remain prompt 091 (as the migration sequence specifies). No capture behavior was changed here.

Final preview completion: the Theme tab now uses a private, muted engine surface and the complete renderer. It creates no physical/network output, uses private video-source identities, and destroys its surface when the tab/window closes. An unavailable preview session retains the basic still fallback and can retry on a later topic update. New regression coverage is written but unexecuted at owner direction; see DECISIONS 075 final close.
