// Builds localized homepages and usage guides into a folder. Release data
// (versions, packages, notes, stars) is fetched here, at build time, so
// visitors' browsers never call the GitHub API. Without network access the page still
// builds, with a link to GitHub Releases instead of the package list.
//
//   node tool/build_site.mjs [out dir]        (default: target/site)
//
// Environment: SITE_URL, SITE_REPOSITORY, GITHUB_TOKEN or GH_TOKEN (optional, raises the
// API rate limit), SITE_REQUIRE_RELEASES=1 (fail without release data instead of building a
// fallback page; used in CI).

import { cpSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { EN_RELEASE_NOTES, STRINGS } from '../site/strings.mjs';
import { GUIDE_PAGES, guideLinks, renderGuide } from '../site/guides.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = resolve(process.argv[2] || join(root, 'target/site'));
const repo = process.env.SITE_REPOSITORY || 'WenWangxin/Tokmira';
const siteUrl = (process.env.SITE_URL || 'https://wenwangxin.github.io/Tokmira/').replace(/\/*$/, '/');
const repoUrl = `https://github.com/${repo}`;

/** Published packages and the platform tab each belongs to, in display order. */
const PACKAGES = [
  { key: 'macos-universal', platform: 'macos', kind: 'DMG', pattern: /-macos-universal\.dmg$/ },
  { key: 'windows-amd64', platform: 'windows', kind: 'EXE', pattern: /-windows-amd64-setup\.exe$/ },
  { key: 'windows-arm64', platform: 'windows', kind: 'EXE', pattern: /-windows-arm64-setup\.exe$/ },
];
const PLATFORMS = ['macos', 'windows'];

/** Release-note headings and the category each maps to; `null` is left out of the page. */
const HEADINGS = [
  [/^(新增|新功能|主要功能|new|added|features?)$/i, 'new'],
  [/^(修复|问题修复|fix(es|ed)?|bug ?fixes)$/i, 'fix'],
  [/^(改进|优化|变更|improve(ments|d)?|changed?)$/i, 'improve'],
  [/^(安装说明|安装|install(ation)?)$/i, null],
  [/^(说明|注意|notes?)$/i, 'note'],
];

const escape = (value) =>
  String(value).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
/** Escaped text with `code` spans. */
const inline = (text) => escape(text).replace(/`([^`]+)`/g, '<code>$1</code>');
const megabytes = (bytes) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;

async function github(path) {
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
  const response = await fetch(`https://api.github.com/${path}`, {
    headers: {
      accept: 'application/vnd.github+json',
      'user-agent': 'tokmira-site-build',
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
  });
  if (!response.ok) throw new Error(`GitHub API ${path}: ${response.status}`);
  return response.json();
}

async function loadReleases() {
  try {
    const list = await github(`repos/${repo}/releases?per_page=50`);
    if (!Array.isArray(list)) throw new Error('GitHub API returned an invalid release list');
    const releases = list
      .filter((release) => !release.draft && release.published_at)
      .sort((a, b) => b.published_at.localeCompare(a.published_at));
    if (!releases.length) throw new Error('GitHub API returned no published releases');
    // A release without an English translation shows its original notes on the English
    // page, so publishing a release never blocks the site.
    const untranslated = releases.filter((release) => !EN_RELEASE_NOTES[release.tag_name]);
    if (untranslated.length) {
      console.warn(
        `warning: no English release notes for ${untranslated.map((release) => release.tag_name).join(', ')} in site/strings.mjs; the English page shows the original notes`,
      );
    }
    let stars = null;
    try {
      const info = await github(`repos/${repo}`);
      stars = info.stargazers_count ?? null;
    } catch (error) {
      console.warn(`warning: no star count (${error.message})`);
    }
    return { stars, releases };
  } catch (error) {
    if (process.env.SITE_REQUIRE_RELEASES === '1') throw error;
    console.warn(`warning: no release data (${error.message}); the page links to GitHub Releases`);
    return { stars: null, releases: [] };
  }
}

function parseNotes(body) {
  const intro = [];
  const sections = [];
  let current = null;
  for (const raw of (body || '').split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || /^(?:token-audit|Tokmira)\b/i.test(line) || /^#+\s*(?:token-audit|Tokmira)\b/i.test(line)) continue;
    const heading = line.replace(/^#+\s*/, '').match(/^(.+?)[：:]$/);
    if (heading) {
      const match = HEADINGS.find(([pattern]) => pattern.test(heading[1].trim()));
      current = { category: match ? match[1] : 'note', items: [] };
      sections.push(current);
      continue;
    }
    if (current?.category === null) continue;
    if (!current) {
      intro.push(line.replace(/^[-*]\s+/, ''));
      continue;
    }
    current.items.push(line.replace(/^[-*]\s+/, ''));
  }
  return { intro, sections: sections.filter((section) => section.category !== null && section.items.length) };
}

function stars(count, lang) {
  if (count === null) return '';
  if (lang === 'zh' && count >= 10000) return `${(count / 10000).toFixed(1).replace(/\.0$/, '')}万`;
  if (lang === 'en' && count >= 1000) return `${(count / 1000).toFixed(1).replace(/\.0$/, '')}k`;
  return String(count);
}

const ICONS = {
  github:
    '<svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8a8 8 0 0 0 5.47 7.59c.4.07.55-.17.55-.38v-1.33c-2.22.48-2.69-1.07-2.69-1.07-.36-.92-.89-1.17-.89-1.17-.73-.5.06-.49.06-.49.8.06 1.23.83 1.23.83.71 1.22 1.87.87 2.33.66.07-.52.28-.87.5-1.07-1.77-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48v2.2c0 .21.15.46.55.38A8 8 0 0 0 16 8c0-4.42-3.58-8-8-8Z"/></svg>',
  theme:
    '<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><circle cx="12" cy="12" r="8.5" fill="none" stroke="currentColor" stroke-width="2"/><path fill="currentColor" d="M12 3.5a8.5 8.5 0 0 1 0 17Z"/></svg>',
  chevron:
    '<svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="m9 6 6 6-6 6"/></svg>',
  download:
    '<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M12 4v12m0 0 5-5m-5 5-5-5M5 20h14"/></svg>',
  arrow:
    '<svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M7 17 17 7M8 7h9v9"/></svg>',
  bulb:
    '<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7V17h8v-2.3A7 7 0 0 0 12 2Z"/></svg>',
  search:
    '<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><circle cx="11" cy="11" r="7" fill="none" stroke="currentColor" stroke-width="2"/><path stroke="currentColor" stroke-width="2" stroke-linecap="round" d="m20 20-3.5-3.5"/></svg>',
};

/** Floating cards beside the hero, each a piece of the app's dashboard with sample numbers. */
function heroCards(t) {
  const c = t.cards;
  const circumference = 2 * Math.PI * 24;
  const remaining = 72;
  return `<div class="floats" aria-hidden="true">
          <div class="float f-ring">
            <svg viewBox="0 0 64 64" width="56" height="56"><circle class="track" cx="32" cy="32" r="24"/><circle class="arc" cx="32" cy="32" r="24" stroke-dasharray="${((circumference * remaining) / 100).toFixed(1)} ${circumference.toFixed(1)}"/></svg>
            <span><small>${escape(c.ringTitle)}</small><b class="value">${remaining}%</b><small>${escape(c.ringLabel)}</small></span>
          </div>
          <div class="float f-advice">
            <span class="ico">${ICONS.bulb}</span>
            <span><b>${escape(c.adviceTitle)}</b><em class="tag">${escape(c.adviceTag)}</em></span>
          </div>
          <div class="float f-prompt">
            <i class="dot claude"></i>
            <span><b>${escape(c.promptTitle)}</b><small>${escape(c.promptMeta)}</small></span>
          </div>
          <div class="float f-weekly" style="--fill: 34%">
            <span class="row"><small>${escape(c.weeklyTitle)}</small><b>34%</b></span>
            <span class="bar"><i></i></span>
          </div>
        </div>`;
}

function render(lang, data) {
  const t = STRINGS[lang];
  const other = lang === 'zh' ? 'en' : 'zh';
  const base = lang === 'zh' ? '' : '../';
  const pageUrl = siteUrl + (lang === 'zh' ? '' : 'en/');
  const otherHref = lang === 'zh' ? 'en/' : '../';
  const latest = data.releases.find((release) => !release.prerelease) || null;
  const version = latest?.tag_name.replace(/^v/, '') || null;
  const date = (iso) => t.dateFormat(new Date(iso));

  const packages = PACKAGES.map((pkg) => {
    const asset = latest?.assets.find((item) => pkg.pattern.test(item.name));
    return asset ? { ...pkg, asset } : null;
  }).filter(Boolean);

  const packageRow = (pkg) => {
    const [name, desc] = t.packages[pkg.key];
    const recommended = pkg.key === 'macos-universal' || pkg.key === 'windows-amd64';
    const sha = pkg.asset.digest?.startsWith('sha256:') ? pkg.asset.digest.slice(7) : '';
    return `
          <li class="pkg${recommended ? ' is-recommended' : ''}" data-key="${pkg.key}" data-platform="${pkg.platform}" data-href="${escape(pkg.asset.browser_download_url)}" data-size="${megabytes(pkg.asset.size)}" data-label="${escape(name)}">
            <span class="kind">${pkg.kind}</span>
            <span class="pkg-name"><b>${escape(name)} <em class="rec">${escape(t.recommended)}</em></b><small>${escape(desc)}</small></span>
            <span class="size">${megabytes(pkg.asset.size)}</span>
            ${sha ? `<button class="sha" type="button" data-sha="${sha}" title="${escape(t.copySha)}">SHA-256</button>` : '<span></span>'}
            <a class="dl" href="${escape(pkg.asset.browser_download_url)}" aria-label="${escape(`${t.navDownload} ${pkg.asset.name}`)}">${ICONS.download}</a>
          </li>`;
  };

  const panels = PLATFORMS.map((platform, index) => {
    const info = t.platforms[platform];
    const rows = packages.filter((pkg) => pkg.platform === platform);
    return `
        <div class="panel" id="panel-${platform}" role="tabpanel" aria-labelledby="tab-${platform}"${index ? ' hidden' : ''}>
          <h3 class="panel-heading">${escape(info.tab)}</h3>
          <p class="hint">${escape(info.hint)}</p>
          ${rows.length ? `<ul class="pkgs">${rows.map(packageRow).join('')}
          </ul>` : `<p class="empty"><a href="${repoUrl}/releases">${escape(t.noRelease)}</a></p>`}
          <div class="card first-open"><b>${escape(info.firstOpenTitle)}</b><p>${escape(info.firstOpen)}</p></div>
        </div>`;
  }).join('');

  const tabs = PLATFORMS.map(
    (platform, index) =>
      `<button class="tab" type="button" role="tab" id="tab-${platform}" data-platform="${platform}" aria-controls="panel-${platform}" aria-selected="${index === 0}" tabindex="${index === 0 ? 0 : -1}">${escape(t.platforms[platform].tab)}</button>`,
  ).join('');

  let year = null;
  const releases = data.releases.map((release, index) => {
    const published = new Date(release.published_at);
    const releaseYear = published.getUTCFullYear();
    const divider = releaseYear !== year ? `<p class="year"><span>${releaseYear}</span></p>` : '';
    year = releaseYear;
    const relVersion = release.tag_name.replace(/^v/, '');
    const { intro, sections } = parseNotes(lang === 'en' ? EN_RELEASE_NOTES[release.tag_name] || release.body : release.body);
    const text = [relVersion, ...intro, ...sections.flatMap((section) => section.items)].join(' ').toLowerCase();
    return `${divider}
          <article class="release reveal" data-text="${escape(text)}">
            <header class="rel-head">
              <h3>${escape(relVersion)}${index === 0 && !release.prerelease ? ` <span class="latest">${escape(t.latest)}</span>` : ''}</h3>
              <time datetime="${release.published_at.slice(0, 10)}">${escape(date(release.published_at))}</time>
              <a href="${escape(release.html_url)}">GitHub ${ICONS.arrow}</a>
            </header>
            <div class="rel-body">${intro.map((paragraph) => `<p class="rel-intro">${inline(paragraph)}</p>`).join('')}${sections.map((section) => `
              <section class="cat cat-${section.category}">
                <h4><i class="cap" aria-hidden="true"></i>${escape(t.categories[section.category])}</h4>
                <ul>${section.items.map((item) => `<li>${inline(item)}</li>`).join('')}</ul>
              </section>`).join('')}
            </div>
          </article>`;
  }).join('');

  const client = { lang, ...t.client, recommended: t.recommended };
  const structured = {
    '@context': 'https://schema.org',
    '@type': 'SoftwareApplication',
    name: 'Tokmira',
    url: pageUrl,
    description: t.description,
    inLanguage: t.htmlLang,
    applicationCategory: 'DeveloperApplication',
    operatingSystem: 'macOS, Windows',
    image: `${siteUrl}preview.png`,
    downloadUrl: `${repoUrl}/releases/latest`,
    ...(version ? { softwareVersion: version, dateModified: latest.published_at.slice(0, 10) } : {}),
    license: 'https://opensource.org/license/mit',
    isAccessibleForFree: true,
    offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
    sameAs: [repoUrl],
  };

  return `<!doctype html>
<html lang="${t.htmlLang}">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${escape(t.title)}</title>
  <meta name="description" content="${escape(t.description)}">
  <meta name="google-site-verification" content="0ffw6JqwWNVPYhtNVF7zFx32Qv9TGC2RQ79tnhEMn1c">
  <meta name="msvalidate.01" content="78F3F454DD693D0BC6140179E3384C85">
  <meta name="color-scheme" content="light dark">
  <meta name="theme-color" media="(prefers-color-scheme: light)" content="#f6f6f9">
  <meta name="theme-color" media="(prefers-color-scheme: dark)" content="#131316">
  <link rel="canonical" href="${pageUrl}">
  <link rel="alternate" hreflang="zh-CN" href="${siteUrl}">
  <link rel="alternate" hreflang="en" href="${siteUrl}en/">
  <link rel="alternate" hreflang="x-default" href="${siteUrl}">
  <meta property="og:type" content="website">
  <meta property="og:title" content="${escape(t.title)}">
  <meta property="og:description" content="${escape(t.description)}">
  <meta property="og:url" content="${pageUrl}">
  <meta property="og:image" content="${siteUrl}preview.png">
  <meta property="og:locale" content="${t.ogLocale}">
  <meta name="twitter:card" content="summary_large_image">
  <link rel="icon" href="${base}icon.svg" type="image/svg+xml">
  <link rel="apple-touch-icon" href="${base}icon.png">
  <link rel="stylesheet" href="${base}assets/style.css">
  <noscript><style>.tabs,.search,.theme{display:none}.panel[hidden],.panel-heading{display:block}</style></noscript>
  <script>
    // Apply the theme before the first paint. Language URLs remain stable; visitors
    // can switch explicitly without being redirected away from a search result.
    (function () {
      var store = {};
      try { store = window.localStorage; } catch (e) {}
      var theme = store.getItem && store.getItem('site-theme');
      if (theme === 'light' || theme === 'dark') document.documentElement.dataset.theme = theme;
      document.documentElement.classList.add('js');
    })();
  </script>
  <script type="application/ld+json">${JSON.stringify(structured)}</script>
</head>
<body>
  <a class="skip" href="#download">${escape(t.skip)}</a>
  <header class="top">
    <div class="wrap top-row">
      <a class="brand" href="./"><img src="${base}icon.svg" width="26" height="26" alt=""><span>Tokmira</span></a>
      <nav class="nav" aria-label="${escape(t.navDownload)}">
        <a href="#download">${escape(t.navDownload)}</a>
        <a href="#guides">${escape(t.navGuides)}</a>
        <a href="#privacy">${escape(t.navPrivacy)}</a>
        <a href="#changelog">${escape(t.navChangelog)}</a>
      </nav>
      <div class="top-actions">
        <a class="stars" href="${repoUrl}" aria-label="GitHub${data.stars > 0 ? ` · ${escape(t.stars)} ${data.stars}` : ''}">${ICONS.github}${data.stars > 0 ? `<span>${stars(data.stars, lang)}</span>` : ''}</a>
        <a class="lang" href="${otherHref}?lang=${other}" hreflang="${STRINGS[other].htmlLang}" lang="${STRINGS[other].htmlLang}" data-lang="${other}" title="${escape(t.otherLangName)}">${escape(t.otherLang)}</a>
        <button class="theme" type="button" aria-label="${escape(t.themeToggle)}">${ICONS.theme}</button>
      </div>
    </div>
    <span class="progress" aria-hidden="true"></span>
  </header>

  <main>
    <section class="hero">
      <div class="wrap hero-copy">
        ${heroCards(t)}
        ${version ? `<a class="release-pill" href="#changelog"><span class="pulse" aria-hidden="true"></span>${escape(t.released(version))}${ICONS.chevron}</a>` : ''}
        <h1>
          <span class="name">Tokmira</span>
          <span class="line">${escape(t.heroBefore)} <span class="reel" data-tool="0"><span class="reel-text">${escape(t.client.tools[0])}</span></span> <span class="after">${escape(t.heroAfter)}</span></span>
        </h1>
        <p class="lede">${escape(t.heroLede)}</p>
        ${version && !packages.some((pkg) => pkg.asset.name.startsWith('tokmira-')) ? `<p class="meta">${escape(t.legacyBrandNotice(version))}</p>` : ''}
        <div class="cta-row">
          <a class="cta" id="cta" href="#download">${ICONS.download}<span><b id="cta-title">${escape(t.client.cta.other)}</b><small id="cta-sub">${version ? `v${escape(version)}` : escape(t.client.ctaFallback)}</small></span></a>
          <a class="quiet" href="#download">${escape(t.other)}</a>
        </div>
      </div>
      <div class="wrap stage reveal">
        <div class="window">
          <div class="titlebar" aria-hidden="true"><i></i><i></i><i></i></div>
          <picture>
            <source media="(prefers-color-scheme: dark)" srcset="${base}preview-dark.png">
            <img class="shot" src="${base}preview.png" data-light="${base}preview.png" data-dark="${base}preview-dark.png" width="1280" height="720" alt="${escape(t.previewAlt)}" decoding="async">
          </picture>
        </div>
        <div class="menubar" aria-hidden="true">
          <img src="${base}icon.svg" width="14" height="14" alt="">
          <span><i class="dot claude"></i>Claude ≈28%</span>
          <span><i class="dot codex"></i>Codex 21%</span>
        </div>
      </div>
      <ul class="wrap facts">${t.facts.map(([title, text], index) => `
        <li class="reveal"><i class="cap c${index + 1}" aria-hidden="true"></i><b>${escape(title)}</b><span>${escape(text)}</span></li>`).join('')}
      </ul>
    </section>

    <section class="section" id="download" aria-labelledby="download-title">
      <div class="wrap split">
        <div class="side reveal">
          <p class="num"><i class="cap" aria-hidden="true"></i>01</p>
          <h2 id="download-title">${escape(t.downloadTitle)}</h2>
          ${version ? `<p class="meta">${escape(t.downloadMeta(version, date(latest.published_at)))}</p>` : ''}
          <p class="note">${escape(t.downloadNote)}</p>
          <p class="links"><a href="${repoUrl}/releases">${escape(t.allReleases)}</a></p>
        </div>
        <div class="main reveal">
          <div class="tabs" role="tablist" aria-label="${escape(t.downloadTitle)}">${tabs}</div>${panels}
        </div>
      </div>
    </section>

    <section class="section" id="privacy" aria-labelledby="privacy-title">
      <div class="wrap split">
        <div class="side reveal">
          <p class="num"><i class="cap" aria-hidden="true"></i>02</p>
          <h2 id="privacy-title">${escape(t.privacyTitle)}</h2>
          <p class="meta">${escape(t.privacyLede)}</p>
        </div>
        <div class="main">
          <ol class="principles">${t.principles.map(([title, text], index) => `
            <li class="reveal"><span class="pn">${index + 1}</span><b>${escape(title)}</b><p>${escape(text)}</p></li>`).join('')}
          </ol>
          <div class="card network reveal">
            <h3>${escape(t.networkTitle)}</h3>
            <ul>${t.network.map(([title, text]) => `
              <li><i class="cap" aria-hidden="true"></i><b>${escape(title)}</b><p>${escape(text)}</p></li>`).join('')}
            </ul>
            <p class="site-note">${escape(t.siteNote)}</p>
          </div>
        </div>
      </div>
    </section>

    <section class="section" id="guides" aria-labelledby="guides-title">
      <div class="wrap split">
        <div class="side reveal">
          <p class="num"><i class="cap" aria-hidden="true"></i>03</p>
          <h2 id="guides-title">${escape(t.guidesTitle)}</h2>
          <p class="meta">${escape(t.guidesIntro)}</p>
        </div>
        <nav class="main guide-links reveal" aria-label="${escape(t.navGuides)}">${guideLinks(lang)}</nav>
      </div>
    </section>

    <section class="section" id="changelog" aria-labelledby="changelog-title">
      <div class="wrap split">
        <div class="side reveal">
          <p class="num"><i class="cap" aria-hidden="true"></i>04</p>
          <h2 id="changelog-title">${escape(t.changelogTitle)}</h2>
          ${data.releases.length ? `<p class="meta">${escape(t.changelogMeta(data.releases.length))}</p>
          <label class="search">${ICONS.search}<input id="search" type="search" placeholder="${escape(t.search)}" aria-label="${escape(t.search)}"><kbd>/</kbd></label>` : ''}
        </div>
        <div class="main log">${releases || `<p class="empty"><a href="${repoUrl}/releases">${escape(t.noRelease)}</a></p>`}
          <p class="no-match" hidden>${escape(t.noMatch)}</p>
        </div>
      </div>
    </section>
  </main>

  <footer class="foot">
    <div class="wrap foot-row">
      <div class="foot-brand"><img src="${base}icon.svg" width="22" height="22" alt=""><div><b>Tokmira</b><small>${escape(t.footer)}</small></div></div>
      <nav class="foot-links" aria-label="GitHub">
        <a href="${repoUrl}">GitHub</a>
        <a href="${repoUrl}/releases">Releases</a>
        <a href="${repoUrl}/issues">${escape(t.issues)}</a>
      </nav>
    </div>
  </footer>

  <script id="site-data" type="application/json">${JSON.stringify(client).replace(/</g, '\\u003c')}</script>
  <script src="${base}assets/app.js" defer></script>
</body>
</html>
`;
}

const data = await loadReleases();
rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'en'), { recursive: true });
cpSync(join(root, 'site/assets'), join(out, 'assets'), { recursive: true });
cpSync(join(root, 'snapshots/preview.png'), join(out, 'preview.png'));
cpSync(join(root, 'snapshots/preview-dark.png'), join(out, 'preview-dark.png'));
cpSync(join(root, 'src-tauri/icons/source/icon.svg'), join(out, 'icon.svg'));
cpSync(join(root, 'src-tauri/icons/128x128@2x.png'), join(out, 'icon.png'));
writeFileSync(join(out, 'index.html'), render('zh', data));
writeFileSync(join(out, 'en/index.html'), render('en', data));
for (const page of GUIDE_PAGES) {
  mkdirSync(join(out, page.path), { recursive: true });
  writeFileSync(join(out, page.path, 'index.html'), renderGuide(page, { siteUrl, repoUrl, strings: STRINGS[page.lang] }));
}
writeFileSync(join(out, '.nojekyll'), '');
writeFileSync(
  join(out, 'sitemap.xml'),
  `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${['', 'en/', ...GUIDE_PAGES.map((page) => page.path)].map((path) => `  <url><loc>${escape(`${siteUrl}${path}`)}</loc></url>`).join('\n')}\n</urlset>\n`,
);
writeFileSync(join(out, 'robots.txt'), `User-agent: *\nAllow: /\nSitemap: ${siteUrl}sitemap.xml\n`);
console.log(`site built in ${out} (${data.releases.length} releases)`);
