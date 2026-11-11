// The clipboard extension must never remember secrets or one-time codes. Extracts looksSecret() from the
// extension source and checks it. Run: node tests/clipboard-filter.js
const fs = require('fs');
const src = fs.readFileSync(__dirname + '/../packaging/gnome-extension/hestia-clipboard@hestia.app/extension.js', 'utf8');
const m = src.match(/function looksSecret[\s\S]*?\n}\n/);
if (!m) { console.error('looksSecret not found'); process.exit(1); }
const looksSecret = new Function(m[0] + '; return looksSecret;')();
const cases = [
  ['123456', true], ['0042', true],
  ['ghp_' + 'aB3'.repeat(12), true], ['github_pat_' + 'x'.repeat(30), true], ['sk-' + 'z'.repeat(30), true],
  ['-----BEGIN OPENSSH PRIVATE KEY-----', true], ['password: hunter2hunter2', true], ['API_KEY=abcdef123456', true],
  ['eyJhbGciOi.eyJzdWIiOi.abc', true], ['q'.repeat(40), true],
  ['hello world', false], ['See you at 6', false], ['https://example.com/some/path', false],
  ['meeting notes for tomorrow about the roadmap', false], ['2026-11-11', false],
];
let bad = 0;
for (const [text, want] of cases) if (looksSecret(text) !== want) { console.error('FAIL', JSON.stringify(text), 'want', want); bad++; }
if (bad) process.exit(1);
console.log('clipboard filter ok');
