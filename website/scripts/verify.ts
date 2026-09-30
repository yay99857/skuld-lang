import { readFile, readdir, mkdtemp, writeFile, rm, access } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
import { marked, type Token, type Tokens } from 'marked';
import { navigation } from '../src/config.js';
import { codeBlock } from '../src/highlight.js';

async function files(dir: string): Promise<string[]> { const entries = await readdir(dir, { withFileTypes: true }); return (await Promise.all(entries.map(e => e.isDirectory() ? files(join(dir, e.name)) : Promise.resolve([join(dir, e.name)])))).flat(); }
const htmlFiles = (await files('dist')).filter(f => f.endsWith('.html'));
const routes = new Map<string, string>();
for (const file of htmlFiles) routes.set('/' + file.replaceAll('\\', '/').replace(/^dist\//, '').replace(/index.html$/, ''), await readFile(file, 'utf8'));
for (const page of navigation) assert(routes.has(page.path), `Missing route ${page.path}`);
let links = 0;
for (const [path, html] of routes) {
  for (const text of ['<html lang="en"', '<title>', 'name="description"', 'rel="canonical"', 'property="og:title"', 'id="main"']) assert(html.includes(text), `${path}: missing ${text}`);
  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map(m => m[1]); assert.equal(ids.length, new Set(ids).size, `${path}: duplicate IDs`);
  for (const match of html.matchAll(/(?:href|src)="([^"]+)"/g)) {
    const url = match[1]; if (!url.startsWith('/') && !url.startsWith('#')) continue;
    const parsed = new URL(url, `https://local.test${path}`); const target = parsed.pathname;
    if (target.startsWith('/assets/') || target.endsWith('.json')) { await access(join('dist', target)); continue; }
    const destination = routes.get(target); assert(destination, `${path}: broken route ${url}`);
    if (parsed.hash) assert(destination.includes(`id="${parsed.hash.slice(1)}"`), `${path}: broken anchor ${url}`);
    links++;
  }
}
for (const [language, sample] of Object.entries({ skuld: 'let n = 42', bash: 'echo "hello"', json: '{"n":42}', toml: 'value = 42', c: 'int n = 42;', cpp: 'int n = 42;', rust: 'let n = 42;', javascript: 'const n = 42;', typescript: 'const n: number = 42;' })) assert(codeBlock(sample, language).includes('class="hljs-'), `Highlighting missing: ${language}`);
const index = JSON.parse(await readFile('dist/search-index.json', 'utf8')) as { path: string; title: string }[];
assert.equal(index.length, navigation.length); index.forEach(item => assert(routes.has(item.path)));
const bin = process.env.SKULD_BIN || resolve('..', 'target', 'debug', process.platform === 'win32' ? 'skuld.exe' : 'skuld');
await access(bin);
const temp = await mkdtemp(join(tmpdir(), 'skuld-docs-'));
let examples = 0;
try {
  for (const file of (await files('content')).filter(f => f.endsWith('.md'))) {
    const source = await readFile(file, 'utf8');
    const tokens: Tokens.Code[] = [];
    marked.walkTokens(marked.lexer(source), (token: Token) => { if (token.type === 'code' && token.lang?.split(' ')[0] === 'skuld') tokens.push(token as Tokens.Code); });
    for (const block of tokens) {
      if (!block.text.includes('func main()') && !block.text.includes('func test_')) continue;
      if (block.text.includes('import "geometry"')) { await import('node:fs/promises').then(fs => fs.mkdir(join(temp, 'geometry'), { recursive: true })); await writeFile(join(temp, 'geometry', 'point.skuld'), 'pub struct Point {\n x: int\n y: int\n}\n'); }
      const filename = join(temp, `example-${examples}.skuld`);
      const checked = block.text.includes('func test_') ? block.text + '\nfunc main() { test_addition() }\n' : block.text;
      await writeFile(filename, checked);
      try { execFileSync(bin, ['check', filename], { encoding: 'utf8', timeout: 30000, stdio: 'pipe' }); }
      catch (error) { throw new Error(`${file}: Skuld example failed:\n${block.text}`, { cause: error }); }
      examples++;
    }
  }
} finally { await rm(temp, { recursive: true, force: true }); }
console.log(`Verified ${htmlFiles.length} static pages, ${links} local links, ${navigation.length} search entries, 9 highlight languages and ${examples} complete Skuld examples.`);
