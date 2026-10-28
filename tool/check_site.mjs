// Check the built site's link graph before publishing, including nested language routes.
// Usage: node tool/check_site.mjs [site directory]
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { GUIDE_PAGES } from '../site/guides.mjs';

const out = resolve(process.argv[2] || 'target/site');
const paths = ['', 'en/', ...GUIDE_PAGES.map((page) => page.path)];
assert.equal(new Set(paths).size, 6, 'Expect two homepages and four distinct guides');
const htmlAt = (path) => readFileSync(join(out, path, 'index.html'), 'utf8');
const canonical = (html) => html.match(/<link rel="canonical" href="([^"]+)"/)[1];
const siteUrl = new URL(canonical(htmlAt('')));
const decode = (text) => text.replace(/&amp;/g, '&').replace(/&quot;/g, '"').replace(/&#39;/g, "'");
const sitemap = readFileSync(join(out, 'sitemap.xml'), 'utf8');
const locations = [...sitemap.matchAll(/<loc>([^<]+)<\/loc>/g)].map((match) => decode(match[1]));
assert.deepEqual(locations.sort(), paths.map((path) => new URL(path, siteUrl).href).sort());
let checkedLinks = 0;

for (const path of paths) {
  const html = htmlAt(path);
  const url = new URL(path, siteUrl);
  assert.equal(canonical(html), url.href);
  assert.equal([...html.matchAll(/<h1(?:\s|>)/g)].length, 1, `${path}: exactly one page heading`);
  assert.match(html, /<meta name="description" content="[^\"]+"/);
  assert.doesNotMatch(html, /location\.replace\(/, `${path}: no forced language redirect`);
  for (const [, json] of html.matchAll(/<script[^>]*type="application\/(?:ld\+)?json"[^>]*>([\s\S]*?)<\/script>/g)) {
    JSON.parse(json);
  }
  for (const [, raw] of html.matchAll(/\b(?:href|src|srcset)="([^"]+)"/g)) {
    const target = new URL(decode(raw), url);
    if (target.origin !== siteUrl.origin) continue;
    assert.ok(target.pathname.startsWith(siteUrl.pathname), `Link leaves the project base: ${raw}`);
    const localPath = decodeURIComponent(target.pathname.slice(siteUrl.pathname.length));
    const file = join(out, localPath, target.pathname.endsWith('/') ? 'index.html' : '');
    assert.ok(existsSync(file), `${path}: missing ${raw}`);
    if (target.hash) {
      const id = decodeURIComponent(target.hash.slice(1));
      assert.ok(readFileSync(file, 'utf8').includes(`id="${id}"`), `${path}: missing anchor ${raw}`);
    }
    checkedLinks++;
  }
  const alternate = path.startsWith('en/') ? path.slice(3) : `en/${path}`;
  const language = path.startsWith('en/') ? 'zh-CN' : 'en';
  assert.ok(html.includes(`hreflang="${language}" href="${new URL(alternate, siteUrl).href}"`), `${path}: matching language counterpart`);
}
for (const path of ['', 'en/']) {
  assert.match(htmlAt(path), /name="google-site-verification"/);
  assert.match(htmlAt(path), /name="msvalidate\.01"/);
}
assert.ok(readFileSync(join(out, 'robots.txt'), 'utf8').includes(`${siteUrl.href}sitemap.xml`));
console.log(`Checked ${paths.length} pages, ${checkedLinks} local links/assets, language pairs and sitemap.`);
