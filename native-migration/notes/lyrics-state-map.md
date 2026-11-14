# Lyrics page state and event map (prompt 080)

Source: `../kairo-presenter/src/renderer/src/components/lyrics/Lyrics.tsx` at
`de3bb54` (4,061 lines, no drift), with `LibraryRail.tsx`, `SongQuickOpen.tsx`,
`SongUsageModal.tsx`, `LyricsReviewNotice.tsx`, `section-colors.ts`,
`stores/useLibraries.ts` and `stores/useSetlist.ts`.

Owner column: **080** = library portion (this prompt), **081** = slide grid
and live push, **082** = editors, import and translate. The native owner of
each 080 row is `ui_lyrics::model::Lyrics` (field or method named).

## 1. State

| Electron state | Kind | Meaning | Owner | Native |
|---|---|---|---|---|
| `songs` | React, seeded from `useBootstrapStore.lyrics` | The library | 080 | `songs` |
| bootstrap merge effect | effect on `bootstrapLyrics` | New songs prepended, newer `updatedAt` replaces held copy, older left alone | 080 | `merge_songs` (feed `Songs`) |
| `loading` | constant `false` | Library always in hand | 080 | none (snapshot is synchronous) |
| `lyricsSettings.glossColor`, `paintColor` | settings, local | Gloss colour, paint mode | 081/082 | — |
| `lyricsSelectedSongId` | `useAppStore`, session-persisted | Selected song | 080 | `selected` (model lives for the session) |
| `lyricsFilter` (`all`/`favorites`/`recent`) | `useAppStore` | Rail source | 080 | `filter` |
| `lyricsSortBy` (`title`/`artist`/`recent`/`added`) | `useAppStore` | List order | 080 | `sort` |
| `lyricsSlideZoom` | `useAppStore` | Slide tile size | 081 | — |
| `viewingSetlistId` | local (not persisted) | Setlist on screen, or library view | 080 | `viewing_setlist` |
| `activeLibraryId` | local, `DEFAULT_LIBRARY_ID` | Library narrowing the pool | 080 | `active_library` |
| `songLibrary` | `useLibrary('songs')` | Library state | 080 | `libraries` (feed `Libraries`) |
| `setlistState` | `useSetlistStore` | Setlists and active id | 080 | `setlists` (feed `Setlists`) |
| `pendingOpenIdRef` | ref | Song to open once it lands in `songs` | 080 | `pending_open` |
| `editMode`, `editState`, `isNewSong`, `saving`, `saveError`, `editBaselineRef` | local | Song editor | 082 | — (`leaveEditor` guard: 082) |
| `translating`, `translateError`, `translateSourceLang` | local | Translate | 082 | — |
| `sendTimerRef`, `sendStatus`, `sendError` | local | Action bar | 081 (`sendError` also shows setlist errors: 080 `error`) | `error` |
| `query` | local, ephemeral | Filter field | 080 | `query` |
| `ftsMatches`, `ftsSearching`, `ftsSeqRef` | local | Debounced (180 ms, ≥2 chars) full-text hits | 080 | `fts`, `fts_searching`, `fts_seq` |
| `onlineRequested`, `onlineSuggestions`, `onlineSearching`, `onlineError`, `onlineSeqRef` | local | Opt-in web search (≥3 chars) | 080 | `online` (`OnlineState`) |
| `importingOnlineId` | local | Web hit being imported | 080 | `importing_online` |
| `webPreviewResult`, `webPreviewData`, `webPreviewLoading`, `webPreviewError`, `webPreviewSeqRef`, `webPreviewCacheRef` | local | Right-pane web preview with cache | 080 | `web_preview` (`WebPreview`), `preview_cache` |
| `sortOpen`, `exportOpen` + refs | local | Menus | 080 (Kit `DropdownMenu` owns open state) | — |
| `libraryWidth` | `useSidebarWidth` | Resizable sidebar | 080 | fixed width (DECISIONS 080) |
| `reorderBusy`, `reorderLock` | local | Slide reorder | 081 | — |
| `contextMenu` | local `{songId,x,y}` | Song row menu | 080 | Kit `ContextMenuExt` |
| `deleteTarget` | local | Delete confirm | 080 | `delete_target` |
| `showImport` | local + `useImportRequest(['lyrics'])` | Import modal | 082 | — |
| `liveSlideIndex`, `liveSongId`, `pushingSlideIndex` | local | Live slide | 081 | — |
| `usageOpen` | local | Song usage dialog | 080 | `usage` (`Usage`) |
| `copiedLyrics` | local | Copy feedback | 081 action bar | — |
| `setlistSelect` (`useMultiSelect`) | hook | Picked songs in setlist view | 080 | `picks` |
| `setlistDropEnd` | local | Drop-at-end highlight | 080 | view `drop_end` |
| slide drag, selecting slides, slide menu, quick edit, follow refs | local | Slide grid | 081 | — |
| LibraryRail: `renamingId`, `draftName`, `dropListId`, `landedListId`, `error`, `songDragging`, `sectionDrop` | local | Setlist section | 080 | `renaming_setlist`, `landed_setlist`, `rail_error`; drop state in the view |
| SongQuickOpen: `query`, `activeIndex`, `onlineResults`, `onlineSearching`, `onlineError`, `onlinePreview`, `previewLoading`, `importingId`, `searchSeq`, `previewSeq`, `previewOpen`, `added`, `dragging` | local | ⌘F finder | 080 | `ui_lyrics::quick_open::QuickOpen` |
| SongUsageModal: `from`, `to` (90 days back to today), `entries`, `busy`, `saved` | local | CCLI report | 080 | `Usage` |

Derived: `selectedSong`, `filteredSongs` (library pool → local title/artist/CCLI
match + FTS hits within the pool → setlist order or filter + sort; `recent`
filter forces `recent` sort), `setlistSongs`, `contextMenuSong`,
`countByLibrary`, favourites count. Native: `rows::filtered_songs`,
`rows::apply_sort`, `collections::library_counts`.

## 2. Events and flows (080)

| Trigger | Electron | Native |
|---|---|---|
| Select library | leave setlist, set library, filter `all` | `select_library` |
| Select Favorites | leave setlist, filter `favorites` | `select_filter` |
| Select setlist | view it; select its first song unless the open one is in it | `select_setlist` |
| Setlist deleted elsewhere | view falls back to library | `apply(Setlists)` |
| Song row click | `handleSelect`: select, drop web preview, clear errors | `select_song` |
| ⌘/⇧-click in setlist | pick | `pick_song` |
| ⌘A / Delete / Esc in setlist list | select all / remove picked (confirm when >1) / clear | `select_all_picks`, `remove_picked` (view confirms), `clear_picks` |
| Row star | `toggleFavorite`, update the song | `toggle_favorite` |
| Row × (setlist) | `removeSong` | `remove_from_setlist` |
| Row ⋯ / right-click | menu: Edit (082), Favorite, Export Song… (`exportKairo songs`), Delete | context menu |
| Delete → confirm | `lyrics.delete`, drop song, clear selection | `ask_delete`, `confirm_delete`, `cancel_delete` |
| Drop song on setlist row | add (or "already on" notice), landed ✓ 1.2 s | `drop_on_setlist` |
| Drop on section (no row) | active list, else create "Sunday service" then add | `drop_on_setlists` |
| Drop on setlist view row/end | `add` at index, own slot discounted | `drop_into_setlist` |
| Drag song onto a library | `libraries.assign` | `LibrarySectionEvent::Assign` |
| ＋ → New playlist | create, view it, rename | `create_setlist` |
| Setlist rename (dbl-click, menu) | trimmed, changed, ≤80 | `begin_rename_setlist`, `commit_rename_setlist`, `cancel_rename_setlist` |
| Setlist menu Export / Delete | `exportKairo setlist` / `delete` | `export_setlist`, `delete_setlist` |
| Library ＋/rename/delete | `libraries.command` | `library_command` |
| List menu Sort by | `setSortBy` | `set_sort` |
| List menu Import .kairo | `importKairo()` | `import_kairo` |
| List menu Export songs / playlist / library | transfer | `export_songs`, `export_setlist`, `export_library` |
| List menu Song usage report | open modal | `open_usage` |
| ＋ New song (`lyrics-new`) | editor | `LyricsEvent::NewSong` (082) |
| Type in filter | local match now, FTS after 180 ms (≥2), web reset | `set_query` |
| Clear × | empty query | `set_query("")` |
| Search online / for more | opt in (≥3 chars) | `search_online` |
| Web hit click | preview in right pane (cache, seq guard, unavailable shortcut) | `preview_suggestion` |
| Web hit Import / Open source / In library | import → pending open; open URL; select existing | `import_suggestion`, `open_existing` |
| Song lands | pending open selects it, drops preview | `merge_songs` / `import_finished` |
| Quick open ⌘F | `SongQuickOpen` overlay | `QuickOpen` + `Overlay::SongSearch` |

## 3. Error, empty and failure paths found

1. Library empty and no query: "No songs yet" + hint.
2. Query 1–2 chars, nothing matches: "Nothing in your library matches that."
3. Query ≥3, nothing matches: same text + "Search online".
4. Query ≥3 with matches: "Search online for more".
5. FTS pending with no local match: "Searching library…".
6. FTS fails: hits dropped silently (local matches stay).
7. A stale FTS answer (older seq) is ignored.
8. Online search fails: red error text (raw message in the page; formatted in quick open).
9. Online search empty: "No matches online." (page) / "Could not find available lyrics…" (quick open).
10. New query drops web hits, the web preview and opt-in.
11. Online answer after a newer search: ignored; preview kept only if still in the results.
12. Web hit with `lyricsAvailable === false`: no fetch, "Found online. Lyrics are not available for automatic import…", Open source opens the URL.
13. Preview fetch fails: `formatOnlineLyricsError` text + "Try another search result, or paste lyrics under Import → Paste."
14. Preview answer after another hit was chosen: ignored (seq).
15. Preview cached per result id.
16. Import of a web hit fails: formatted error in the web section; busy cleared.
17. Import buttons disabled while any import runs.
18. Web hit already in library: "In library" opens the local copy.
19. Online review notice: reviewed (with "Unrelated page text removed"), unverified missing-key / too-large / other.
20. Toggle favourite or delete rejected: Electron unhandled; native shows the error.
21. Deleting the selected song clears the selection.
22. Selection of a song no longer known is dropped, except while a pending open waits.
23. Setlist command fails: rail error line (`role=alert`).
24. Dropping a song already on a list: notice "“T” is already on L", order unchanged.
25. No setlists: section hidden unless a song is being dragged ("Drop here to start a playlist").
26. Rename empty or unchanged: no command; Esc cancels.
27. Removing more than one picked song: confirm "Remove N songs from L? They stay in your library."
28. Viewed setlist deleted: back to the library.
29. Setlist with songs missing from the library: missing ids skipped.
30. Usage: no pushes in range "No songs were pushed in this period."; missing CCLI count; Export disabled when empty or busy; export failure alert; saved path shown; dialog cancelled saves nothing.
31. Usage list fails: logged, entries kept.
32. Quick open: nothing typed shows the whole library A–Z; "N songs"/"N matches"; "Not in your library" + error or hint; "Searching the web…"; web search only on request and only ≥3 chars; Enter with no row searches the web; ⌘Enter on a web row opens (imports); adding with no setlist creates "Sunday service"; a song with no sections "This song has no lyrics saved yet."; Esc and backdrop close.
