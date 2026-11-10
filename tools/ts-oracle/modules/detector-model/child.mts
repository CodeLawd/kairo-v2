// Capture the SDKs' actual bodies after Electron's managedFetch adapter.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';

const require = createRequire(join(process.cwd(), 'package.json'));
require('electron-log/main').transports.console.level = false;

const importMain = (path: string) =>
  import(pathToFileURL(join(process.cwd(), 'src/main/services', path)).href);
const { ScriptureDetector } = await importMain('scripture/detector.ts');
const { managedTransport } = await importMain('cloud/managed.ts');

const cases = JSON.parse(readFileSync(0, 'utf8'));
const results = [];
for (const scenario of cases) {
  const requests: unknown[] = [];
  managedTransport.request = async (request: unknown) => {
    requests.push(request);
    return scenario.response;
  };
  const detector = new ScriptureDetector({
    managed: true,
    apiKey: 'kairo-managed',
    provider: scenario.provider,
    model: scenario.model,
    minIntervalMs: 0,
  });
  const detections: unknown[] = [];
  detector.on('detection', (refs: any[]) => {
    for (const r of refs) {
      detections.push([
        r.book, r.chapter, r.verseStart, r.verseEnd ?? null,
        r.detectionType, r.confidence, r.sourceText, r.translations ?? [],
      ]);
    }
  });
  try {
    await new Promise<void>((resolve, reject) => {
      detector.once('stats', () => resolve());
      detector.once('error', reject);
      detector.beginTranscript({ sttReceivedAt: 500, source: 'final' }, scenario.text);
      detector.analyze(scenario.text);
    });
    results.push({ requests, detections });
  } finally {
    detector.destroy();
  }
}
process.stdout.write(JSON.stringify(results));
