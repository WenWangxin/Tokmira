// Product guides are static, translated content. No usage logs or visitor data are read.
export const GUIDE_PAGES = ['claude-code', 'codex'].flatMap((tool) => ['zh', 'en'].map((lang) => ({
  tool, lang, path: `${lang === 'en' ? 'en/' : ''}${tool}-usage/`,
})));

const COPY = {
  zh: {
    home: '首页', guides: '用量指南', download: '下载应用', privacy: '隐私与数据来源', otherLanguage: 'English',
    eyebrow: '用量指南', sectionSteps: '先看额度，再找消耗', sectionNumbers: '三个数字，分别理解',
    sectionExample: '把一次异常，追到具体会话', sectionLimits: '哪些数据可以解释', sectionFaq: '常见问题',
    preview: '界面示意 · 内置演示数据，非真实用户记录',
    headings: ['数字', '它说明什么', '它不说明什么'],
    numbers: [
      ['Token 用量', '本机日志记录的输入、输出和缓存 token。', '不等于账号剩余额度，也不是实际账单。'],
      ['订阅额度', '带来源和时间的官方读数、记录或估算。', '不能按固定比例把每个 token 换成额度。'],
      ['API 等价费用', '按随应用附带的模型价格表折算的金额。', '不是订阅额外收费，也不是平台实际扣款。'],
    ],
    exampleLabel: '操作示例 · 不承诺固定节省比例', exampleSteps: ['在用量明细中选一个窗口', '按会话或项目查看消耗排名', '打开会话，核对时间线与诊断', '调整用法后，再比较相近任务'],
    exampleNote: '比较时同时看模型、上下文长度、任务规模和数据来源。单次变化不足以证明优化有效。',
    sourceTitle: '数据来源', next: '继续阅读', ctaTitle: '用自己的使用记录，找到答案',
    ctaText: '免费、MIT 开源，支持 macOS 和 Windows。默认在本机处理用量，不上传对话内容。',
    faqText: '应用能解释本机编程工具的用量；平台是否收取费用，应以官方账单为准。',
  },
  en: {
    home: 'Home', guides: 'Usage guides', download: 'Download the app', privacy: 'Privacy and data sources', otherLanguage: '简体中文',
    eyebrow: 'Usage guide', sectionSteps: 'Check your quota. Then follow the usage.', sectionNumbers: 'Three numbers, three different meanings',
    sectionExample: 'Follow a costly pattern back to its session', sectionLimits: 'Understand the coverage', sectionFaq: 'Frequently asked questions',
    preview: 'UI preview · built-in demo data, not a real user record. This screenshot shows the Chinese interface.',
    headings: ['Number', 'What it tells you', 'What it does not tell you'],
    numbers: [
      ['Token usage', 'Input, output and cache tokens recorded in local logs.', 'Your remaining account quota or an actual invoice.'],
      ['Subscription quota', 'An official reading, recorded snapshot or estimate, with its source and time.', 'A fixed token allowance or a universal tokens-to-quota conversion.'],
      ['API-equivalent cost', 'The usage valued at the model prices bundled with the app.', 'An extra subscription charge or the amount actually billed.'],
    ],
    exampleLabel: 'Illustrative workflow · no fixed savings claim', exampleSteps: ['Choose a window in Usage', 'Rank usage by session or project', 'Open a session and inspect its timeline and diagnoses', 'Change one habit, then compare similar tasks'],
    exampleNote: 'Compare model, context length, task size and data source as well as totals. A single change does not establish that an optimization worked.',
    sourceTitle: 'Data sources', next: 'Read next', ctaTitle: 'Find the answer in your own usage',
    ctaText: 'Free and MIT-licensed, for macOS and Windows. Usage is processed locally by default; conversation content is not uploaded.',
    faqText: 'The app explains local coding-tool usage. For actual charges, refer to the provider’s billing records.',
  },
};

const CONTENT = {
  'claude-code': {
    product: 'Claude Code', other: 'codex',
    sourceUrl: 'https://code.claude.com/docs/en/costs', sourceName: 'Claude Code: Manage costs effectively',
    zh: {
      title: 'Claude Code 用量统计与额度监控 | token-audit',
      heading: 'Claude Code 用量统计与额度监控',
      description: '查看 Claude Code 的额度、重置时间和本机 token 消耗，按项目、会话与提示定位用量，理解缓存重建与长上下文。免费开源，支持 macOS 和 Windows。',
      lead: '知道还剩多少，也看清为什么用得快。把额度读数、本机 token 和会话诊断放在一起，找到下一次可以调整的用法。',
      steps: [
        ['查看当前额度', '在 Claude Code 中运行 `/usage`，先查看账号的额度与重置时间。要持续查看，可打开 token-audit 仪表盘，并核对每个数字的来源与更新时间。'],
        ['选择数据来源', 'Claude 精确额度可通过主动开启官方查询，或安装命令行状态栏记录器获得。未开启时可手动校准；日志里的 token 本身不能直接还原官方剩余额度。'],
        ['定位高消耗会话', '打开“用量明细”，选择一个额度窗口，按项目、会话、模型或子代理查看。提示级归因需要相应的本机记录；估算会单独标明。'],
        ['阅读原因与建议', '查看缓存过期重建、长上下文、模型切换或高价模型等诊断。打开会话时间线核对发生位置，再按任务需要决定是否调整。'],
      ],
      scenarioTitle: '长时间离开后，恢复会话突然消耗较多',
      scenario: '在会话时间线里查看恢复前后的请求。如果诊断命中缓存重建，结合记录中的缓存时长和缓存写入量理解原因。保留必要上下文、集中完成相关工作，都是可以比较的做法。',
      limits: [
        ['官方额度是账号级数据', '官方查询可能包含其他设备、Claude 聊天等活动。本机日志无法归属的差额显示为“未归属”，不会猜测是哪个会话造成的。'],
        ['本机明细只覆盖 Claude Code', '支持命令行及桌面版的 Code 功能，不统计普通网页聊天、桌面聊天或 Cowork 的 token 明细。'],
        ['查询与记录器由你选择', '官方查询只向 Anthropic 发送登录令牌，令牌仅短暂保留在内存中。记录器安装前备份配置，保留原有状态栏和 hooks，只记录用量与事件元数据。'],
      ],
      faqs: [
        ['Claude Code 用量在哪里看？', '当前状态先看 `/usage`。token-audit 用于跨会话查看本机历史、消耗分布和诊断，并在菜单栏或托盘持续展示额度。'],
        ['额度百分比是精确的吗？', '官方读数与记录快照对应各自的采集时刻。两次读数之间的推算显示 ≈；提示和会话的额度归因也有数据覆盖限制，应同时查看来源说明。'],
        ['为什么短提示也会消耗较多？', '一次提示可能触发多次模型请求，每次还会携带上下文。新会话的工具定义、缓存写入和子代理也可能增加用量；具体原因需要看本机诊断。'],
        ['能告诉我实际多扣了多少钱吗？', '当前版本提供 token 统计、额度归因和 API 等价费用，不做中转站逐条账单对账，也不能仅凭价差判定多扣费。'],
      ],
    },
    en: {
      title: 'Claude Code Usage Monitor & Token Tracker | token-audit',
      heading: 'Claude Code usage monitor and token tracker',
      description: 'Track Claude Code quota, reset times and local token usage. Find costly sessions, projects and prompts, and understand cache rebuilds. Free, open-source app for macOS and Windows.',
      lead: 'See what is left, and understand what used it. Bring quota readings, local token counts and session diagnoses together before changing how you work.',
      steps: [
        ['Check the current quota', 'Run `/usage` in Claude Code to check account limits and reset times. For an ongoing view, open the token-audit dashboard and read the source and update time beside each number.'],
        ['Choose your data source', 'For exact Claude quota readings, explicitly enable the official query or install the CLI status-line recorder. Manual calibration is also available. Token counts alone cannot reconstruct the official remaining quota.'],
        ['Find the costly sessions', 'In Usage, choose a quota window and compare projects, sessions, models or subagents. Prompt attribution needs the corresponding local records; estimates are labeled.'],
        ['Inspect the diagnosis', 'Review patterns such as expired-cache rebuilds, long context, model switches or expensive models. Open the session timeline to check where the pattern appeared, then decide what fits your task.'],
      ],
      scenarioTitle: 'A session becomes costly after a long break',
      scenario: 'Compare requests before and after resuming in the session timeline. If cache rebuilding is diagnosed, check the recorded cache lifetime and write tokens. Keeping only necessary context or completing related work together are habits you can compare on similar tasks.',
      limits: [
        ['Official quota is account-wide', 'An official reading may include other devices and Claude chat activity. Differences that local records cannot explain remain unattributed; the app does not guess which session caused them.'],
        ['Detailed usage covers Claude Code', 'Terminal sessions and the desktop Code feature are supported. Ordinary web or desktop chats and Cowork do not have local token breakdowns in this app.'],
        ['Queries and recording are optional', 'The official query sends the login token only to Anthropic and holds it briefly in memory. Recorder installation backs up configuration and preserves existing status lines and hooks. It records usage and event metadata, not prompts.'],
      ],
      faqs: [
        ['How do I check Claude Code usage?', 'Start with `/usage` for current status. token-audit adds local history across sessions, usage breakdowns and diagnoses, plus a persistent quota view in the menu bar or tray.'],
        ['Are quota percentages exact?', 'Official readings and recorded snapshots describe their collection time. Estimates between readings use ≈. Per-prompt and per-session attribution has coverage limits too, so read the source information with the number.'],
        ['Why can a short prompt consume substantial quota?', 'A prompt can trigger multiple model requests, each carrying context. Tool definitions at startup, cache writes and subagents may also add usage. Check the local diagnosis for your particular session.'],
        ['Does the app prove that I was overcharged?', 'No. This version provides token statistics, quota attribution and API-equivalent estimates. It does not reconcile individual relay invoices or establish overcharging from a price difference.'],
      ],
    },
  },
  codex: {
    product: 'Codex', other: 'claude-code',
    sourceUrl: 'https://developers.openai.com/codex/pricing/', sourceName: 'Codex: Pricing and usage limits',
    zh: {
      title: 'Codex 用量统计、额度查询与监控 | token-audit',
      heading: 'Codex 用量统计与额度查询',
      description: '查看 Codex 本机 token 用量、可用额度窗口和重置时间，按项目、会话与模型分析消耗，定位长上下文和高推理用量。免费开源，支持 macOS 和 Windows。',
      lead: '把当前额度和过去的消耗连起来。查看本机 Codex 的请求记录，找到最费额度的项目、会话和任务模式。',
      steps: [
        ['先确认账号与额度来源', '打开 token-audit，完成首次引导并确认检测到了本机 Codex。查看额度卡片的来源、时间和账号套餐；窗口是否可用以实际返回的数据为准。'],
        ['查看剩余额度与重置时间', '仪表盘显示可用的五小时和每周窗口、重置时间以及触顶预测。数据可来自本机日志快照或 Codex 本地接口；没有足够数据时不会补造精确值。'],
        ['按项目和会话拆分用量', '在“用量明细”中选择一个窗口，比较项目、会话和模型。再打开具体会话，查看每次请求的 token、缓存和额度变化。'],
        ['检查上下文和推理消耗', '查看长上下文、高推理占比、单次提示触发多轮请求等诊断。根据任务需要调整模型、推理强度或任务范围，再对比后续的相近任务。'],
      ],
      scenarioTitle: '同一个项目里，某个会话明显比其他会话更费',
      scenario: '先在会话列表定位它，再查看请求时间线和诊断标签。区分是输入上下文变长、推理 token 增多，还是一次提示触发了更多请求。不要仅根据输出字数判断消耗。',
      limits: [
        ['本机记录有覆盖边界', '支持 Codex 命令行和桌面端留下的本机使用记录。其他设备或未保存在本机的活动可能无法归属到具体项目或会话。'],
        ['套餐上限不是固定 token 数', '模型、任务和套餐会影响额度。应用展示返回的窗口和来源，不承诺每个套餐都有相同的五小时或每周额度。'],
        ['默认不读取账号密钥', '用量分析读取本机日志；Codex 官方额度可通过本地接口查询。无需把 API Key 或对话内容上传给 token-audit 的服务器。'],
      ],
      faqs: [
        ['Codex 额度在哪里看？', '可在 Codex 自带的用量界面查看账号状态。token-audit 将可用的额度读数和本机历史放在同一处，并提供项目与会话拆分。'],
        ['为什么 token 总量与额度百分比不同步？', '它们的单位和范围不同。Token 是本机请求用量，额度是账号窗口的读数；更新间隔、其他设备和任务差异也会影响两者的对应关系。'],
        ['推理 token 是否算了两遍？', '应用会按日志格式归一化推理字段；推理包含在输出中的记录不会再重复加到输出总量。诊断中的推理比例用于解释消耗结构。'],
        ['可以直接给我增加或重置额度吗？', '不能。应用用于监控与分析，不修改官方额度，不绕过平台限制，也不代替官方计费页面。'],
      ],
    },
    en: {
      title: 'Codex Usage Tracker & Quota Monitor | token-audit',
      heading: 'Codex usage tracker and quota monitor',
      description: 'Track local Codex token usage, available quota windows and reset times. Compare projects, sessions and models, and inspect context and reasoning usage. Free and open source for macOS and Windows.',
      lead: 'Connect the quota you see now with the work you did earlier. Follow local Codex requests back to the projects, sessions and task patterns that used them.',
      steps: [
        ['Confirm the account and data source', 'Open token-audit and complete the first-run guide to detect local Codex records. Check the source, timestamp and plan on the quota cards. Available windows depend on the data returned for your account.'],
        ['Check quota and reset times', 'The dashboard shows available five-hour and weekly windows, reset times and a limit forecast. Readings can come from local log snapshots or the Codex local interface. Missing data is not presented as an exact value.'],
        ['Break usage down by project and session', 'Choose a window in Usage and compare projects, sessions and models. Open a session to inspect each request’s tokens, cache usage and quota changes on a timeline.'],
        ['Review context and reasoning', 'Look for diagnoses such as long context, a high reasoning share or many requests from one prompt. Adjust the model, reasoning effort or task scope to suit the work, then compare similar tasks.'],
      ],
      scenarioTitle: 'One session costs more than others in the same project',
      scenario: 'Locate the session, then inspect its request timeline and diagnosis tags. Distinguish growing input context from more reasoning tokens or more requests per prompt. Output length on its own does not describe the full consumption.',
      limits: [
        ['Local records have a coverage boundary', 'Codex terminal and desktop activity recorded on this machine is supported. Activity on another device or not retained locally may not be attributable to a project or session.'],
        ['Plan limits are not fixed token counts', 'Model, task and plan affect quota. The app displays the windows and sources it receives; it does not assume every plan has the same five-hour or weekly limits.'],
        ['No account keys required for local analysis', 'Usage analysis reads local logs; official Codex quota can be queried through its local interface. You do not upload an API key or conversation content to a token-audit server.'],
      ],
      faqs: [
        ['How do I check Codex usage?', 'Use Codex’s own usage view for account status. token-audit puts available quota readings beside local history and adds project and session breakdowns.'],
        ['Why do token counts and quota percentages change differently?', 'They measure different things and may cover different activity. Tokens describe local requests; quota readings describe account windows. Update intervals, other devices and task differences also affect the relationship.'],
        ['Are reasoning tokens counted twice?', 'The app normalizes reasoning fields according to the log format. When reasoning is included in output, it is not added to output totals again. Reasoning-share diagnoses explain the composition of that usage.'],
        ['Can this tool increase or reset my Codex limit?', 'No. It monitors and explains usage. It does not alter official limits, bypass restrictions or replace the provider’s billing page.'],
      ],
    },
  },
};

const escape = (text) => String(text).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const inline = (text) => escape(text).replace(/`([^`]+)`/g, '<code>$1</code>');

export function guideLinks(lang, home = './') {
  return ['claude-code', 'codex'].map((tool) => `<a class="guide-link" href="${home}${tool}-usage/"><span>${escape(CONTENT[tool].product)}</span><b>${escape(CONTENT[tool][lang].heading)}</b><span aria-hidden="true">↗</span></a>`).join('');
}

export function renderGuide(page, { siteUrl, repoUrl, strings }) {
  const { lang, tool, path } = page;
  const c = COPY[lang]; const content = CONTENT[tool]; const t = content[lang];
  const base = lang === 'en' ? '../../' : '../';
  const home = '../';
  const url = `${siteUrl}${path}`;
  const alternate = `${lang === 'en' ? '../../' : '../en/'}${tool}-usage/`;
  const zhUrl = `${siteUrl}${tool}-usage/`; const enUrl = `${siteUrl}en/${tool}-usage/`;
  const client = { lang, ...strings.client, tools: [content.product] };
  const schema = {
    '@context': 'https://schema.org', '@type': 'WebPage', name: t.heading, description: t.description,
    url, inLanguage: lang === 'zh' ? 'zh-CN' : 'en',
    isPartOf: { '@type': 'WebSite', name: 'token-audit', url: siteUrl },
    breadcrumb: { '@type': 'BreadcrumbList', itemListElement: [
      { '@type': 'ListItem', position: 1, name: c.home, item: `${siteUrl}${lang === 'en' ? 'en/' : ''}` },
      { '@type': 'ListItem', position: 2, name: t.heading, item: url },
    ] },
  };
  return `<!doctype html>
<html lang="${lang === 'zh' ? 'zh-CN' : 'en'}">
<head>
  <meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${escape(t.title)}</title><meta name="description" content="${escape(t.description)}">
  <meta name="color-scheme" content="light dark">
  <link rel="canonical" href="${url}">
  <link rel="alternate" hreflang="zh-CN" href="${zhUrl}"><link rel="alternate" hreflang="en" href="${enUrl}"><link rel="alternate" hreflang="x-default" href="${enUrl}">
  <meta property="og:type" content="website"><meta property="og:title" content="${escape(t.title)}"><meta property="og:description" content="${escape(t.description)}"><meta property="og:url" content="${url}"><meta property="og:image" content="${siteUrl}preview.png"><meta name="twitter:card" content="summary_large_image">
  <link rel="icon" href="${base}icon.svg" type="image/svg+xml"><link rel="stylesheet" href="${base}assets/style.css">
  <script>(function(){try{var t=localStorage.getItem('site-theme');if(t==='light'||t==='dark')document.documentElement.dataset.theme=t;}catch(e){}})();</script>
  <script type="application/ld+json">${JSON.stringify(schema).replace(/</g, '\\u003c')}</script>
</head>
<body class="guide-page" data-tool="${tool}">
  <a class="skip" href="#steps">${escape(c.sectionSteps)}</a>
  <header class="top"><div class="wrap top-row">
    <a class="brand" href="${home}"><img src="${base}icon.svg" width="26" height="26" alt=""><span>token-audit</span></a>
    <nav class="nav" aria-label="${escape(c.guides)}"><a href="${home}#guides">${escape(c.guides)}</a><a href="${home}#download">${escape(c.download)}</a></nav>
    <div class="top-actions"><a class="lang" href="${alternate}" data-lang="${lang === 'zh' ? 'en' : 'zh'}" hreflang="${lang === 'zh' ? 'en' : 'zh-CN'}">${escape(c.otherLanguage)}</a><button class="theme" type="button" aria-label="${escape(strings.themeToggle)}">◐</button></div>
  </div><span class="progress" aria-hidden="true"></span></header>
  <main>
    <section class="wrap guide-hero">
      <p class="guide-eyebrow"><i class="cap" aria-hidden="true"></i>${escape(c.eyebrow)} <span>/ ${escape(content.product)}</span></p>
      <h1>${escape(t.heading)}</h1><p class="guide-lead">${escape(t.lead)}</p>
      <div class="cta-row"><a class="cta" href="${home}#download">${escape(c.download)} <span aria-hidden="true">↗</span></a><a class="quiet" href="${home}#privacy">${escape(c.privacy)}</a></div>
    </section>
    <figure class="wrap guide-preview"><div class="window"><picture><source media="(prefers-color-scheme: dark)" srcset="${base}preview-dark.png"><img class="shot" src="${base}preview.png" data-light="${base}preview.png" data-dark="${base}preview-dark.png" width="2880" height="1800" alt="${escape(strings.previewAlt)}" decoding="async"></picture></div><figcaption>${escape(c.preview)}</figcaption></figure>
    <div class="wrap guide-layout">
      <aside class="guide-toc" aria-label="${escape(c.guides)}"><a href="#steps">01 ${escape(c.sectionSteps)}</a><a href="#numbers">02 ${escape(c.sectionNumbers)}</a><a href="#example">03 ${escape(c.sectionExample)}</a><a href="#coverage">04 ${escape(c.sectionLimits)}</a><a href="#faq">05 ${escape(c.sectionFaq)}</a></aside>
      <div class="guide-body">
        <section id="steps"><p class="num">01</p><h2>${escape(c.sectionSteps)}</h2><ol class="guide-steps">${t.steps.map(([title, text]) => `<li><h3>${escape(title)}</h3><p>${inline(text)}</p></li>`).join('')}</ol></section>
        <section id="numbers"><p class="num">02</p><h2>${escape(c.sectionNumbers)}</h2><div class="guide-table-wrap"><table><thead><tr>${c.headings.map((h) => `<th scope="col">${escape(h)}</th>`).join('')}</tr></thead><tbody>${c.numbers.map(([name, meaning, limit]) => `<tr><th scope="row">${escape(name)}</th><td>${escape(meaning)}</td><td>${escape(limit)}</td></tr>`).join('')}</tbody></table></div></section>
        <section id="example"><p class="num">03</p><h2>${escape(c.sectionExample)}</h2><div class="guide-scenario"><p class="guide-eyebrow">${escape(c.exampleLabel)}</p><h3>${escape(t.scenarioTitle)}</h3><p>${escape(t.scenario)}</p><ol>${c.exampleSteps.map((step) => `<li>${escape(step)}</li>`).join('')}</ol></div><p class="guide-note">${escape(c.exampleNote)}</p></section>
        <section id="coverage"><p class="num">04</p><h2>${escape(c.sectionLimits)}</h2>${t.limits.map(([title, text]) => `<div class="guide-limit"><h3>${escape(title)}</h3><p>${escape(text)}</p></div>`).join('')}<p class="guide-source">${escape(c.sourceTitle)}: <a href="${content.sourceUrl}" target="_blank" rel="noopener noreferrer">${escape(content.sourceName)} ↗</a> · <a href="${repoUrl}">GitHub</a></p></section>
        <section id="faq"><p class="num">05</p><h2>${escape(c.sectionFaq)}</h2>${t.faqs.map(([q, a]) => `<details class="guide-faq"><summary>${escape(q)}</summary><p>${escape(a)}</p></details>`).join('')}<p class="guide-note">${escape(c.faqText)}</p></section>
        <section class="guide-final"><h2>${escape(c.ctaTitle)}</h2><p>${escape(c.ctaText)}</p><a class="cta" href="${home}#download">${escape(c.download)} ↗</a></section>
        <nav class="guide-next" aria-label="${escape(c.next)}"><span>${escape(c.next)}</span><a href="../${content.other}-usage/">${escape(CONTENT[content.other][lang].heading)} →</a></nav>
      </div>
    </div>
  </main>
  <footer class="foot"><div class="wrap foot-row"><a class="brand" href="${home}">token-audit</a><small>${escape(strings.footer)}</small><nav class="foot-links"><a href="${repoUrl}">GitHub</a><a href="${repoUrl}/issues">${escape(strings.issues)}</a></nav></div></footer>
  <script id="site-data" type="application/json">${JSON.stringify(client).replace(/</g, '\\u003c')}</script><script src="${base}assets/app.js" defer></script>
</body></html>`;
}
