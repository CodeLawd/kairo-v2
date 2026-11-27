// Generates SVG files for the icons the Electron renderer imports from '@/icons'.
const fs = require('fs'), path = require('path');
const { createRequire } = require('module');
const [root, usedFile, outDir] = process.argv.slice(2);
const req = createRequire(path.join(root, 'package.json'));
const React = req('react'); const { renderToStaticMarkup } = req('react-dom/server');
const huge = req('@hugeicons/core-free-icons'); const { HugeiconsIcon } = req('@hugeicons/react');
const phDir = path.join(root, 'node_modules/@phosphor-icons/react/dist/defs');
const phSvg = (glyph, weight) => {
  const t = fs.readFileSync(path.join(phDir, glyph + '.es.js'), 'utf8');
  const start = t.indexOf('"' + weight + '"');
  const end = t.indexOf(']', start);
  const ds = [...t.slice(start, end).matchAll(/d: "([^"]+)"/g)].map(m => m[1]);
  if (!ds.length) throw new Error('no paths ' + glyph + ' ' + weight);
  return '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor">' + ds.map(d => '<path d="' + d + '"/>').join('') + '</svg>';
};
const src = fs.readFileSync(path.join(root, 'src/renderer/src/icons/index.tsx'), 'utf8');
// import aliases
const alias = {};
for (const m of src.matchAll(/(\w+) as (\w+)/g)) alias[m[2]] = m[1];
const def = {};
for (const m of src.matchAll(/export const (\w+) = (wrap|wrapPhosphor)\((\w+)\)/g)) def[m[1]] = { kind: m[2], glyph: alias[m[3]] || m[3] };
for (const m of src.matchAll(/export const (\w+) = (\w+)\s*$/gm)) if (def[m[2]]) def[m[1]] = def[m[2]];
// Native-only glyphs drawn for Kairo, kept whether or not Electron imports them.
const NATIVE = ['Looks', 'PenNib', 'Shapes', 'Sticker', 'Stop'];
const used = [...new Set([...fs.readFileSync(usedFile, 'utf8').split('\n').filter(Boolean).filter(n => n !== 'Icon'), ...NATIVE])];
const snake = n => n.replace(/([a-z0-9])([A-Z])/g, '$1_$2').replace(/([A-Z])([A-Z][a-z])/g, '$1_$2').toLowerCase();
fs.mkdirSync(outDir, { recursive: true });
const clean = s => s.replace(/ class="[^"]*"/g, '').replace(/ (width|height)="[^"]*"/g, '').replace(/^<svg(?![^>]*xmlns)/, '<svg xmlns="http://www.w3.org/2000/svg"').replace(/ color="currentColor"/, '');
const bible = {
  regular: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="none" stroke="currentColor" stroke-width="16" stroke-linecap="round" stroke-linejoin="round"><path d="M64 40h128a24 24 0 0 1 24 24v136a24 24 0 0 1-24 24H64V40Z"/><path d="M64 40v184"/><path d="M128 68v96M96 100h64"/></svg>',
  fill: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256"><path fill="currentColor" fill-rule="evenodd" clip-rule="evenodd" d="M64 40h128a24 24 0 0 1 24 24v136a24 24 0 0 1-24 24H64V40Zm8 0v184h8V40Zm72 24h-16v36h-28v16h28v52h16v-52h28v-16h-28V64Z"/></svg>',
};
// Looks: glasses and a moustache, for choosing how each screen looks.
const looks = {
  regular: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 256 256\"><g fill=\"none\" stroke=\"currentColor\" stroke-width=\"16\" stroke-linecap=\"round\"><circle cx=\"86\" cy=\"96\" r=\"30\"/><circle cx=\"170\" cy=\"96\" r=\"30\"/><path d=\"M116 96q12-12 24 0\"/></g><path fill=\"currentColor\" d=\"M128 156c-16-18-46-22-66-4-10 9-22 10-32 2 4 26 34 40 62 30 16-5 28-14 36-22 8 8 20 17 36 22 28 10 58-4 62-30-10 8-22 7-32-2-20-18-50-14-66 4Z\"/></svg>",
  fill: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 256 256\"><g fill=\"none\" stroke=\"currentColor\" stroke-width=\"20\" stroke-linecap=\"round\"><circle cx=\"86\" cy=\"96\" r=\"30\"/><circle cx=\"170\" cy=\"96\" r=\"30\"/><path d=\"M116 96q12-12 24 0\"/></g><path fill=\"currentColor\" d=\"M128 156c-16-18-46-22-66-4-10 9-22 10-32 2 4 26 34 40 62 30 16-5 28-14 36-22 8 8 20 17 36 22 28 10 58-4 62-30-10 8-22 7-32-2-20-18-50-14-66 4Z\"/></svg>",
};
// The theme editor's insert toolbar: Shapes, Pen and Stickers.
const shapes = {"regular":"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 256 256\"><defs><mask id=\"m\"><rect width=\"256\" height=\"256\" fill=\"#fff\"/><rect x=\"92\" y=\"92\" width=\"148\" height=\"148\" rx=\"34\" fill=\"#000\"/></mask></defs><g fill=\"none\" stroke=\"currentColor\" stroke-width=\"16\"><circle cx=\"96\" cy=\"96\" r=\"60\" mask=\"url(#m)\"/><rect x=\"112\" y=\"112\" width=\"108\" height=\"108\" rx=\"22\"/></g></svg>","fill":"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 256 256\"><defs><mask id=\"m\"><rect width=\"256\" height=\"256\" fill=\"#fff\"/><rect x=\"94\" y=\"94\" width=\"144\" height=\"144\" rx=\"34\" fill=\"#000\"/></mask></defs><circle cx=\"96\" cy=\"96\" r=\"68\" fill=\"currentColor\" mask=\"url(#m)\"/><rect x=\"110\" y=\"110\" width=\"112\" height=\"112\" rx=\"24\" fill=\"currentColor\"/></svg>"};
const penNib = {"regular":"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 256 256\"><g transform=\"rotate(45 128 128)\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"16\" stroke-linejoin=\"round\" stroke-linecap=\"round\"><path d=\"M128 20 186 120 150 200H106L70 120Z\"/><path d=\"M128 20V110\"/><circle cx=\"128\" cy=\"128\" r=\"16\"/><path d=\"M100 236H156\"/></g></svg>","fill":"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 256 256\"><defs><mask id=\"m\"><rect width=\"256\" height=\"256\" fill=\"#fff\"/><g transform=\"rotate(45 128 128)\" fill=\"#000\"><circle cx=\"128\" cy=\"128\" r=\"16\"/><rect x=\"122\" y=\"14\" width=\"12\" height=\"104\"/></g></mask></defs><g transform=\"rotate(45 128 128)\" fill=\"currentColor\"><path mask=\"url(#m)\" d=\"M128 16 192 120 154 204H102L64 120Z\"/><rect x=\"98\" y=\"218\" width=\"60\" height=\"18\" rx=\"6\"/></g></svg>"};
const sticker = {"regular":"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 256 256\"><g fill=\"none\" stroke=\"currentColor\" stroke-width=\"16\" stroke-linejoin=\"round\"><path d=\"M64 40H192A24 24 0 0 1 216 64V148L148 216H64A24 24 0 0 1 40 192V64A24 24 0 0 1 64 40Z\"/><path d=\"M148 216V172A24 24 0 0 1 172 148H216\"/></g><path fill=\"currentColor\" d=\"M118 150c-24-16-38-30-38-46 0-12 9-21 20-21 8 0 14 4 18 11 4-7 10-11 18-11 11 0 20 9 20 21 0 16-14 30-38 46Z\"/></svg>","fill":"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 256 256\"><defs><mask id=\"m\"><rect width=\"256\" height=\"256\" fill=\"#fff\"/><path fill=\"#000\" d=\"M118 150c-24-16-38-30-38-46 0-12 9-21 20-21 8 0 14 4 18 11 4-7 10-11 18-11 11 0 20 9 20 21 0 16-14 30-38 46Z\"/><path fill=\"none\" stroke=\"#000\" stroke-width=\"12\" d=\"M148 216V172A24 24 0 0 1 172 148H216\"/></mask></defs><path mask=\"url(#m)\" fill=\"currentColor\" d=\"M64 40H192A24 24 0 0 1 216 64V148L148 216H64A24 24 0 0 1 40 192V64A24 24 0 0 1 64 40Z\"/><path fill=\"currentColor\" d=\"M216 160 160 216V180A20 20 0 0 1 180 160Z\"/></svg>"};
// Hugeicons' Square01Icon (Electron's Square) draws x², not a square.
const square = {"regular":"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\"><rect x=\"3.5\" y=\"3.5\" width=\"17\" height=\"17\" rx=\"3.5\" stroke=\"currentColor\" stroke-width=\"1.5\"/></svg>","fill":"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\"><rect x=\"3.5\" y=\"3.5\" width=\"17\" height=\"17\" rx=\"3.5\" stroke=\"currentColor\" stroke-width=\"2\"/></svg>"};
// A solid rounded square: stop, end.
const stop = {"regular":"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"><rect x=\"5\" y=\"5\" width=\"14\" height=\"14\" rx=\"3\" fill=\"currentColor\"/></svg>","fill":"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"><rect x=\"5\" y=\"5\" width=\"14\" height=\"14\" rx=\"3\" fill=\"currentColor\"/></svg>"};
const custom = { Bible: bible, Looks: looks, PenNib: penNib, Shapes: shapes, Square: square, Sticker: sticker, Stop: stop };
const manifest = [];
const canonical = {}; // dedupe aliases (CheckIcon = Check)
for (const name of used.sort()) {
  let d = def[name];
  if (custom[name]) d = { kind: 'kairo', glyph: name };
  if (!d && name.startsWith('Ph')) d = { kind: 'wrapPhosphor', glyph: name.slice(2) };
  if (!d) { console.error('unmapped', name); process.exitCode = 1; continue; }
  const key = d.kind + ':' + d.glyph;
  if (canonical[key]) { manifest.push([name, canonical[key], d]); continue; }
  const file = snake(name); canonical[key] = file;
  let regular, fill;
  if (d.kind === 'kairo') ({ regular, fill } = custom[d.glyph]);
  else if (d.kind === 'wrap') {
    regular = clean(renderToStaticMarkup(React.createElement(HugeiconsIcon, { icon: huge[d.glyph], size: 24 })));
    fill = clean(renderToStaticMarkup(React.createElement(HugeiconsIcon, { icon: huge[d.glyph], size: 24, strokeWidth: 2 })));
  } else {
    regular = phSvg(d.glyph, 'regular');
    fill = phSvg(d.glyph, 'fill');
  }
  fs.writeFileSync(path.join(outDir, file + '.svg'), regular + '\n');
  fs.writeFileSync(path.join(outDir, file + '-fill.svg'), fill + '\n');
  manifest.push([name, file, d]);
}
fs.writeFileSync(path.join(outDir, '..', 'icons.json'), JSON.stringify(manifest.map(([n, f, d]) => ({ name: n, file: f, source: d.kind === 'wrap' ? 'hugeicons:' + d.glyph : d.kind === 'wrapPhosphor' ? 'phosphor:' + d.glyph : 'kairo:' + d.glyph })), null, 1) + '\n');

// Rust table: one variant per distinct glyph, named after the Electron export.
const seen = new Set();
const rows = manifest.filter(([n, f]) => { if (seen.has(f)) return false; seen.add(f); return true; });
const variant = n => n;
const lines = [];
lines.push('// @generated by crates/kairo-ui-kit/tools/generate-icons.cjs. Do not edit.');
lines.push('// Source: ../kairo-presenter/src/renderer/src/icons/index.tsx, limited to the');
lines.push('// exports the renderer imports. Hugeicons free (MIT) and Phosphor (MIT).');
lines.push('');
lines.push('/// Every icon the operator interface uses, named as the Electron icon module names it.');
lines.push('#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]');
lines.push('pub enum KairoIcon {');
for (const [n, f, d] of rows) lines.push(`    /// \`${d.kind === 'wrap' ? 'hugeicons ' + d.glyph : d.kind === 'wrapPhosphor' ? 'phosphor ' + d.glyph : 'Kairo ' + d.glyph}\``, `    ${variant(n)},`);
lines.push('}', '', 'impl KairoIcon {');
lines.push('    /// Every icon, in name order.');
lines.push('    pub const ALL: &[KairoIcon] = &[');
for (const [n] of rows) lines.push(`        KairoIcon::${variant(n)},`);
lines.push('    ];', '');
lines.push('    /// The file stem under `kairo/icons/`.');
lines.push('    pub fn file_stem(self) -> &\'static str {', '        match self {');
for (const [n, f] of rows) lines.push(`            KairoIcon::${variant(n)} => "${f}",`);
lines.push('        }', '    }', '');
lines.push('    /// The SVG bytes for one weight.');
lines.push('    pub(crate) fn svg(self, fill: bool) -> &\'static [u8] {', '        match (self, fill) {');
for (const [n, f] of rows) {
  lines.push(`            (KairoIcon::${variant(n)}, false) => include_bytes!("../../assets/icons/${f}.svg"),`);
  lines.push(`            (KairoIcon::${variant(n)}, true) => include_bytes!("../../assets/icons/${f}-fill.svg"),`);
}
lines.push('        }', '    }', '}', '');
// Electron exports several names for one glyph; keep them as aliases.
const aliases = manifest.filter(([n, f]) => !rows.some(([rn]) => rn === n));
if (aliases.length) {
  lines.push('/// Other names the Electron icon module exports for the same glyphs.');
  lines.push('#[allow(non_upper_case_globals)]', 'impl KairoIcon {');
  for (const [n, f] of aliases) {
    const target = rows.find(([, rf]) => rf === f)[0];
    lines.push(`    /// Same glyph as [\`KairoIcon::${target}\`].`, `    pub const ${n}: KairoIcon = KairoIcon::${target};`);
  }
  lines.push('}', '');
}
fs.mkdirSync(path.join(outDir, '..', '..', 'src', 'icons'), { recursive: true });
fs.writeFileSync(path.join(outDir, '..', '..', 'src', 'icons', 'generated.rs'), lines.join('\n'));
