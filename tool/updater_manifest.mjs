import { readFileSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const targets = [
  { suffix: '-macos-universal.app.tar.gz', platforms: ['darwin-aarch64', 'darwin-x86_64'], bundle: 'app' },
  { suffix: '-windows-amd64-setup.exe', platforms: ['windows-x86_64'], bundle: 'nsis' },
  { suffix: '-windows-arm64-setup.exe', platforms: ['windows-aarch64'], bundle: 'nsis' },
];

function requireValue(condition, message) {
  if (!condition) throw new Error(message);
}

export function buildManifest({ assets, signatures, repo, tag, notes, pubDate }) {
  requireValue(/^v\d+\.\d+\.\d+$/.test(tag), 'Invalid release tag');
  requireValue(/^[\w.-]+\/[\w.-]+$/.test(repo), 'Invalid repository');
  requireValue(typeof notes === 'string', 'Release notes must be text');
  requireValue(typeof pubDate === 'string' && !Number.isNaN(Date.parse(pubDate)), 'Invalid release date');
  requireValue(Array.isArray(assets), 'Assets must be a list');
  requireValue(signatures && typeof signatures === 'object', 'Signatures must be a map');

  const names = assets.map((asset) => asset.name);
  requireValue(names.every((name) => typeof name === 'string'), 'Invalid asset name');
  requireValue(new Set(names).size === names.length, 'Duplicate asset name');
  const assetNames = new Set(names);
  const version = tag.slice(1);
  const prefix = `https://github.com/${repo}/releases/download/${tag}/`;
  const platforms = {};

  for (const target of targets) {
    const matches = names.filter((name) => name.startsWith('tokmira-') && name.endsWith(target.suffix));
    requireValue(matches.length === 1, `Expected one ${target.suffix} asset, found ${matches.length}`);
    const name = matches[0];
    requireValue(name.slice('tokmira-'.length, -target.suffix.length) === version, `Asset version does not match ${tag}: ${name}`);
    const sigName = `${name}.sig`;
    requireValue(assetNames.has(sigName), `Missing signature asset: ${sigName}`);
    const signature = signatures[sigName];
    requireValue(typeof signature === 'string' && signature.trim().length > 0, `Missing signature content: ${sigName}`);
    const entry = { signature: signature.trim(), url: `${prefix}${name}` };
    for (const platform of target.platforms) {
      platforms[platform] = entry;
      platforms[`${platform}-${target.bundle}`] = entry;
    }
  }

  const manifest = { version, notes, pub_date: new Date(pubDate).toISOString(), platforms };
  validateManifest(manifest, assetNames, repo, tag);
  return manifest;
}

export function validateManifest(manifest, assetNames, repo, tag) {
  requireValue(manifest.version === tag.slice(1), 'Manifest version does not match tag');
  const prefix = `https://github.com/${repo}/releases/download/${tag}/`;
  requireValue(Object.keys(manifest.platforms).length === 8, 'Manifest must contain eight platform keys');
  for (const target of targets) {
    for (const platform of target.platforms) {
      const main = manifest.platforms[platform];
      const alias = manifest.platforms[`${platform}-${target.bundle}`];
      requireValue(main && alias && main.url === alias.url && main.signature === alias.signature, `Missing or mismatched platform alias: ${platform}`);
      requireValue(typeof main.signature === 'string' && main.signature.length > 0, `Missing signature: ${platform}`);
      requireValue(typeof main.url === 'string' && main.url.startsWith(prefix), `Non-public download URL: ${platform}`);
      requireValue(assetNames.has(main.url.slice(prefix.length)), `URL is not a release asset: ${platform}`);
    }
  }
}

function main() {
  const [releasePath, assetsPath, signaturesDir, outputPath] = process.argv.slice(2);
  requireValue(releasePath && assetsPath && signaturesDir && outputPath, 'Usage: node tool/updater_manifest.mjs RELEASE_JSON ASSETS_JSON SIGNATURES_DIR OUTPUT_JSON');
  const release = JSON.parse(readFileSync(releasePath, 'utf8'));
  const assets = JSON.parse(readFileSync(assetsPath, 'utf8'));
  const signatures = {};
  for (const asset of assets) {
    if (typeof asset.name === 'string' && asset.name.endsWith('.sig')) {
      requireValue(asset.name === basename(asset.name) && !asset.name.includes('\\'), 'Invalid signature asset name');
      signatures[asset.name] = readFileSync(join(signaturesDir, asset.name), 'utf8');
    }
  }
  const manifest = buildManifest({
    assets,
    signatures,
    repo: process.env.GITHUB_REPOSITORY,
    tag: process.env.GITHUB_REF_NAME,
    notes: release.body ?? '',
    pubDate: release.created_at,
  });
  writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`);
  process.stdout.write(`Verified latest.json ${manifest.version}: ${Object.keys(manifest.platforms).join(', ')}\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
