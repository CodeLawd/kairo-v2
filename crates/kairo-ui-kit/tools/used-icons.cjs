const fs=require('fs'),path=require('path');
const root=process.argv[2];
const used=new Set();
(function walk(d){for(const f of fs.readdirSync(d)){const p=path.join(d,f);const s=fs.statSync(p);if(s.isDirectory())walk(p);else if(/\.(tsx?|jsx?)$/.test(f)&&!p.includes('/icons/index.tsx')){const t=fs.readFileSync(p,'utf8');for(const m of t.matchAll(/import\s*(?:type\s*)?\{([^}]*)\}\s*from\s*['"]@\/icons['"]/g)){for(const n of m[1].split(',')){const nm=n.trim().split(/\s+as\s+/)[0].replace(/^type\s+/,'');if(nm)used.add(nm)}}}}})(root);
console.log([...used].sort().join('\n'));
