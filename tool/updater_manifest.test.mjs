import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { buildManifest, validateManifest } from './updater_manifest.mjs';

const repo = 'WenWangxin/Tokmira';
const tag = 'v1.3.1';
const names = [
  'tokmira-1.3.1-macos-universal.app.tar.gz',
  'tokmira-1.3.1-windows-amd64-setup.exe',
  'tokmira-1.3.1-windows-arm64-setup.exe',
];

function input(assetNames = names) {
  const assets = assetNames.flatMap((name) => [{ name }, { name: `${name}.sig` }]);
  const signatures = Object.fromEntries(assetNames.map((name) => [`${name}.sig`, `fake signature for ${name}\n`]));
  return { assets, signatures, repo, tag, notes: 'Release notes\nSecond line', pubDate: '2026-10-29T12:00:00Z' };
}

test('generates all main and bundle keys with public asset URLs', () => {
  const args = input();
  const manifest = buildManifest(args);
  assert.equal(manifest.version, '1.3.1');
  assert.equal(manifest.notes, args.notes);
  assert.equal(manifest.pub_date, '2026-10-29T12:00:00.000Z');
  assert.equal(Object.keys(manifest.platforms).length, 8);
  for (const [main, alias] of [
    ['darwin-aarch64', 'darwin-aarch64-app'],
    ['darwin-x86_64', 'darwin-x86_64-app'],
    ['windows-x86_64', 'windows-x86_64-nsis'],
    ['windows-aarch64', 'windows-aarch64-nsis'],
  ]) {
    assert.deepEqual(manifest.platforms[main], manifest.platforms[alias]);
    assert.match(manifest.platforms[main].url, /^https:\/\/github\.com\/WenWangxin\/Tokmira\/releases\/download\/v1\.3\.1\//);
    assert.ok(args.assets.some((asset) => manifest.platforms[main].url.endsWith(`/${asset.name}`)));
  }
  assert.equal(manifest.platforms['darwin-aarch64'].url, manifest.platforms['darwin-x86_64'].url);
});

test('CLI writes a manifest from local release assets and signature files', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tokmira-manifest-'));
  try {
    const args = input();
    const signaturesDir = join(dir, 'signatures');
    mkdirSync(signaturesDir);
    writeFileSync(join(dir, 'release.json'), JSON.stringify({ body: args.notes, created_at: args.pubDate }));
    writeFileSync(join(dir, 'assets.json'), JSON.stringify(args.assets));
    for (const [name, signature] of Object.entries(args.signatures)) {
      writeFileSync(join(signaturesDir, name), signature);
    }
    const output = join(dir, 'latest.json');
    const result = spawnSync(process.execPath, [fileURLToPath(new URL('./updater_manifest.mjs', import.meta.url)), join(dir, 'release.json'), join(dir, 'assets.json'), signaturesDir, output], {
      env: { ...process.env, GITHUB_REPOSITORY: repo, GITHUB_REF_NAME: tag },
      encoding: 'utf8',
    });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(readFileSync(output, 'utf8')), buildManifest(args));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('rejects a missing platform or signature', () => {
  assert.throws(() => buildManifest(input(names.slice(0, 2))), /windows-arm64/);
  const args = input();
  args.assets.pop();
  assert.throws(() => buildManifest(args), /Missing signature asset/);
});

test('rejects wrong file names and versions', () => {
  const wrongSuffix = input([names[0], names[1], 'tokmira-1.3.1-windows-arm64.exe']);
  assert.throws(() => buildManifest(wrongSuffix), /windows-arm64-setup\.exe/);
  const wrongVersion = input([names[0], names[1], 'tokmira-1.3.0-windows-arm64-setup.exe']);
  assert.throws(() => buildManifest(wrongVersion), /Asset version does not match/);
});

test('rejects a non-public or absent download URL', () => {
  const args = input();
  const manifest = buildManifest(args);
  manifest.platforms['windows-aarch64'] = { ...manifest.platforms['windows-aarch64'], url: 'https://api.github.com/asset/123' };
  assert.throws(() => validateManifest(manifest, new Set(args.assets.map((asset) => asset.name)), repo, tag), /mismatched platform alias/);
  manifest.platforms['windows-aarch64-nsis'].url = manifest.platforms['windows-aarch64'].url;
  assert.throws(() => validateManifest(manifest, new Set(args.assets.map((asset) => asset.name)), repo, tag), /Non-public download URL/);
});
