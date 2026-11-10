// Electron-as-Node oracle for the live detector with no model or quote index.
import { readFileSync } from 'node:fs';
import { mock } from 'node:test';
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';

const root = process.cwd();
const scripture = (file: string) =>
  import(pathToFileURL(join(root, 'src/main/services/scripture', file)).href);
const { ScriptureDetector } = await scripture('detector.ts');
const { subscribeExplicitScriptureDetection } = await scripture('live-detection-wiring.ts');
delete process.env.KAIRO_STT_REPLAY_PATH;

const { cases, quotes } = JSON.parse(readFileSync(0, 'utf8'));
const results = cases.map((events: any[]) => {
  mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 0 });
  const detector = new ScriptureDetector({ apiKey: '', managed: false });
  detector.setChapterVerseProvider((book: string, chapter: number) =>
    quotes.filter((verse: any) => verse.book === book && verse.chapter === chapter));
  detector.setQuoteSearchProvider(() => quotes);
  let final: (event: any) => void;
  let interim: (event: any) => void;
  const unsubscribe = subscribeExplicitScriptureDetection({
    onTranscript(listener: typeof final) { final = listener; },
    onInterim(listener: typeof interim) { interim = listener; },
    offTranscript() {},
    offInterim() {},
  }, detector);
  const found: unknown[] = [];
  detector.on('detection', (refs: any[]) => {
    for (const r of refs) {
      found.push([
        r.book, r.chapter, r.verseStart, r.verseEnd ?? null,
        r.transcriptSource, r.sttReceivedAt, Date.now(), r.translations ?? [], r.resolver, r.confidence, r.sourceText,
      ]);
    }
  });
  function advance(target: number) {
    // Tick one virtual millisecond at a time so timer callbacks observe their
    // due time even when the transcript has a long silence.
    for (let now = Date.now(); now < target; now++) mock.timers.tick(1);
  }
  try {
    for (const [id, event] of events.entries()) {
      advance(event.atMs);
      const receive = event.type === 'interim' ? interim! : final!;
      receive({ ...event, id: String(id), words: [], timestamp: Date.now(), duration: 0 });
    }
    advance(Date.now() + 5_001);
    return found;
  } finally {
    unsubscribe();
    detector.destroy();
    mock.timers.reset();
  }
});
process.stdout.write(JSON.stringify(results));
