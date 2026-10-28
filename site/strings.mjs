// Every piece of text on the site, per language. `tool/build_site.mjs` fills the page with
// these at build time; the `client` part is also embedded for `assets/app.js`.

export const STRINGS = {
  zh: {
    htmlLang: 'zh-CN',
    ogLocale: 'zh_CN',
    title: 'token-audit：看清 Claude Code 和 Codex 的额度用在哪里',
    description:
      '免费开源的桌面工具。读取本机 Claude Code 与 Codex 日志，显示 5 小时和每周额度、额度去向和省额度建议，数据只留在本机。支持 macOS 和 Windows。',
    skip: '跳到下载',
    navDownload: '下载',
    navPrivacy: '隐私',
    navChangelog: '更新日志',
    otherLang: 'EN',
    otherLangName: 'English',
    themeToggle: '切换浅色或深色',
    stars: '在 GitHub 上的 Star 数',
    released: (version) => `v${version} 已发布 · 查看更新`,
    heroBefore: '看清',
    heroAfter: '的额度',
    heroLede:
      '读取本机日志，显示 5 小时和每周额度、每一次提示用掉了多少，以及哪些用法最费额度。数据只留在本机。',
    other: '其他平台',
    cards: {
      ringTitle: 'Claude · 5 小时',
      ringLabel: '剩余',
      adviceTitle: '缓存过期后的整段重建',
      adviceTag: '多耗 3.9% 周额度',
      promptTitle: '「重构登录模块」',
      promptMeta: '一次提示 +4.2% 额度 · 18 万 token',
      weeklyTitle: 'Codex · 本周额度',
    },
    previewAlt: 'token-audit 仪表盘：Claude 与 Codex 的额度、今日用量、额度走势和省额度建议',
    facts: [
      ['额度现状', '5 小时与每周额度、重置时间和触顶预测。'],
      ['额度去哪了', '按会话、项目、模型和单次提示拆分消耗。'],
      ['省额度建议', '识别 16 类浪费，写明下次怎么避免。'],
      ['数据只在本机', '不上传对话内容，核心逻辑以 MIT 协议开源。'],
    ],
    downloadTitle: '下载',
    downloadMeta: (version, date) => `版本 ${version} · ${date} 发布`,
    downloadNote: '已为当前设备选好安装包。安装包暂未签名，首次打开请按下方提示操作。',
    allReleases: '在 GitHub 查看全部版本',
    noRelease: '还没有发布版本，请稍后再来。',
    recommended: '推荐',
    copySha: '复制 SHA-256',
    copied: '已复制',
    platforms: {
      macos: {
        tab: 'macOS',
        hint: 'Apple 芯片和 Intel 的 Mac 都用这一个安装包。',
        firstOpenTitle: '首次打开',
        firstOpen: '在 Finder 里右键应用，选择「打开」。如果仍被拦截，到「系统设置 → 隐私与安全性」里点「仍要打开」。',
      },
      windows: {
        tab: 'Windows',
        hint: '在「设置 → 系统 → 系统信息」里可以查看处理器架构。支持 Windows 10 和 11。',
        firstOpenTitle: '首次运行',
        firstOpen: '出现「Windows 已保护你的电脑」时，点「更多信息」，再点「仍要运行」。',
      },
    },
    packages: {
      'macos-universal': ['通用安装包', 'Apple 芯片与 Intel 的 Mac'],
      'windows-amd64': ['x64', '大多数 Windows 电脑'],
      'windows-arm64': ['ARM64', '骁龙等 ARM 架构的笔记本'],
    },
    privacyTitle: '隐私',
    privacyLede: '这个工具靠信任才有用，所以有三条不会改的原则。',
    principles: [
      ['默认数据不出本机', '只读取日志里的 token 数、模型和时间。对话内容、令牌和密钥不读、不存、不上传。'],
      ['核心逻辑开源', '统计、归属和诊断的算法都在 GitHub 上，结论可以自己复现。'],
      ['结论不受付费影响', '任何人付费都不能改变诊断结果。'],
    ],
    networkTitle: '只在你开启时联网',
    network: [
      ['Claude 官方额度查询', '只发往 Anthropic。登录令牌只在内存里使用，不保存、不刷新。'],
      ['检查更新', '只从 GitHub Releases 下载更新清单和安装包，不附带版本号、设备信息或使用数据。'],
    ],
    siteNote: '这个网站本身也不使用统计脚本、cookie 或外部字体。',
    changelogTitle: '更新日志',
    changelogMeta: (count) => `共 ${count} 个版本`,
    search: '搜索更新内容或版本',
    latest: '最新',
    noMatch: '没有找到相关内容。',
    categories: { new: '新增', fix: '修复', improve: '改进', note: '说明' },
    footer: 'MIT 协议开源 · 数据只留在本机',
    issues: '反馈问题',
    dateFormat: (date) => `${date.getUTCFullYear()}年${date.getUTCMonth() + 1}月${date.getUTCDate()}日`,
    client: {
      tools: ['Claude Code', 'Codex'],
      cta: { macos: '下载 macOS 版', windows: '下载 Windows 版', other: '前往下载' },
      ctaFallback: 'GitHub Releases',
      copied: '已复制',
      copySha: '复制 SHA-256',
    },
  },
  en: {
    htmlLang: 'en',
    ogLocale: 'en_US',
    title: 'token-audit: see where your Claude Code and Codex quota goes',
    description:
      'A free, open-source desktop app. It reads your local Claude Code and Codex logs to show your 5-hour and weekly quota, where it went and how to save it. Your data stays on your machine. For macOS and Windows.',
    skip: 'Skip to downloads',
    navDownload: 'Download',
    navPrivacy: 'Privacy',
    navChangelog: 'Changelog',
    otherLang: '中文',
    otherLangName: '简体中文',
    themeToggle: 'Switch between light and dark',
    stars: 'Stars on GitHub',
    released: (version) => `v${version} is out · See what's new`,
    heroBefore: 'Know your',
    heroAfter: 'quota',
    heroLede:
      'It reads your local logs to show your 5-hour and weekly quota, what each prompt used and which habits cost the most. Your data stays on your machine.',
    other: 'Other platforms',
    cards: {
      ringTitle: 'Claude · 5-hour',
      ringLabel: 'left',
      adviceTitle: 'Cache rebuilt after it expired',
      adviceTag: '+3.9% of weekly quota',
      promptTitle: '“Refactor the login module”',
      promptMeta: 'One prompt +4.2% quota · 180K tokens',
      weeklyTitle: 'Codex · this week',
    },
    previewAlt: 'token-audit dashboard: Claude and Codex quota, today\'s usage, the quota trend and saving advice',
    facts: [
      ['Quota at a glance', '5-hour and weekly quota, reset times and a forecast.'],
      ['Where it went', 'Usage by session, project, model and single prompt.'],
      ['Saving advice', '16 kinds of waste, each with how to avoid it next time.'],
      ['Stays on your machine', 'No conversation content uploaded; the core is MIT-licensed.'],
    ],
    downloadTitle: 'Download',
    downloadMeta: (version, date) => `Version ${version} · released ${date}`,
    downloadNote:
      'The package for this device is picked for you. The packages are not signed yet; see below for the first launch.',
    allReleases: 'All versions on GitHub',
    noRelease: 'No release yet. Please check back later.',
    recommended: 'Recommended',
    copySha: 'Copy SHA-256',
    copied: 'Copied',
    platforms: {
      macos: {
        tab: 'macOS',
        hint: 'One package for both Apple silicon and Intel Macs.',
        firstOpenTitle: 'First launch',
        firstOpen:
          'Right-click the app in Finder and choose Open. If it is still blocked, choose Open Anyway in System Settings → Privacy & Security.',
      },
      windows: {
        tab: 'Windows',
        hint: 'Settings → System → About shows your processor type. Windows 10 and 11 are supported.',
        firstOpenTitle: 'First run',
        firstOpen: 'If "Windows protected your PC" appears, choose More info, then Run anyway.',
      },
    },
    packages: {
      'macos-universal': ['Universal', 'Macs with Apple silicon or Intel'],
      'windows-amd64': ['x64', 'Most Windows PCs'],
      'windows-arm64': ['ARM64', 'Snapdragon and other ARM laptops'],
    },
    privacyTitle: 'Privacy',
    privacyLede: 'A tool like this is only useful if you can trust it, so three principles do not change.',
    principles: [
      ['Your data stays on your machine', 'Only token counts, models and times are read. Conversation content, tokens and keys are never read, stored or uploaded.'],
      ['The core is open source', 'The counting, attribution and diagnosis code is on GitHub, so you can reproduce every result.'],
      ['Results cannot be bought', 'Nobody can pay to change a diagnosis.'],
    ],
    networkTitle: 'Online only when you turn it on',
    network: [
      ['Claude official quota query', 'Sent only to Anthropic. Your login token is kept in memory only, never saved or refreshed.'],
      ['Checking for updates', 'Downloads the manifest and packages only from GitHub Releases, without your version, device details or usage data.'],
    ],
    siteNote: 'This website uses no analytics, cookies or external fonts either.',
    changelogTitle: 'Changelog',
    changelogMeta: (count) => `${count} ${count === 1 ? 'release' : 'releases'}`,
    search: 'Search changes or versions',
    latest: 'Latest',
    noMatch: 'Nothing matches.',
    categories: { new: 'New', fix: 'Fixes', improve: 'Improvements', note: 'Notes' },
    footer: 'Open source under MIT · Your data stays on your machine',
    issues: 'Report an issue',
    dateFormat: (date) =>
      date.toLocaleDateString('en-US', { year: 'numeric', month: 'short', day: 'numeric', timeZone: 'UTC' }),
    client: {
      tools: ['Claude Code', 'Codex'],
      cta: { macos: 'Download for macOS', windows: 'Download for Windows', other: 'Get token-audit' },
      ctaFallback: 'GitHub Releases',
      copied: 'Copied',
      copySha: 'Copy SHA-256',
    },
  },
};

// Release bodies are authored in Chinese on GitHub. Keep the English page complete by
// translating each published release here before the Pages workflow deploys it.
export const EN_RELEASE_NOTES = {
  'v1.1.0': `token-audit 1.1.0

New:
- The interface now supports Chinese and English. Choose Follow system, Chinese or English in Settings; the tray menu and window close notice update immediately. Dates and token counts use the selected language's format.
- The menu bar can show the 5-hour quota for Claude and Codex, up to two values stacked. The tray menu has been reorganized with Menu bar display and Launch mode submenus.
- Between official quota readings, new local usage is used to estimate the current quota, marked ≈ on the dashboard. When Claude's official quota query is enabled, new usage also triggers a fresh query.

Notes:
- Saving advice details are available in Chinese only for now; English is planned for the next release.
- The command line interface is still in Chinese.

Installation:
- The packages are not code signed or notarized.
- macOS: Right-click the app in Finder and choose Open, or allow it in System Settings → Privacy & Security.
- Windows: If SmartScreen appears, choose More info → Run anyway.`,
  'v1.0.0': `token-audit 1.0.0

The first stable release for Claude Code and Codex subscribers. See where your quota goes and why it runs out.

Features:
- Dashboard: 5-hour and weekly quota, reset times, a limit forecast, subscription value and a shareable card.
- Usage details by session, project, subagent and model.
- Saving advice detects costly patterns such as rebuilding an expired cache, overlong context and using an expensive model for a light task.
- Session list and details.
- Optional recorder with a status line and hooks. It preserves existing settings and never records prompt content.
- Background saving reminders, a persistent menu bar or tray icon, and launch at login.
- In-app update checks and automatic installation.
- All data is processed on your machine; API keys, login tokens and conversation content are never uploaded.

Installation:
- The packages are not code signed or notarized.
- macOS: Right-click the app in Finder and choose Open, or allow it in System Settings → Privacy & Security.
- Windows: If SmartScreen appears, choose More info → Run anyway.`,
};
