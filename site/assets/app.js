// Page behaviour. Everything here works on what the build already put in the page; the
// script makes no network requests and stores only the visitor's theme and language choice.
(function () {
  'use strict';

  var data = JSON.parse(document.getElementById('site-data').textContent);
  var root = document.documentElement;
  var reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  function store(key, value) {
    try { window.localStorage.setItem(key, value); } catch (e) { /* private mode: not remembered */ }
  }

  // --- Platform: pick the tab, the recommended package and the hero button. ---
  function detect() {
    var ua = navigator.userAgent;
    var hint = navigator.userAgentData && navigator.userAgentData.platform;
    // Phones and tablets say "like Mac OS X" or "Linux"; there is nothing to install there.
    if (/iPhone|iPad|iPod|Android/i.test(ua) || (navigator.userAgentData && navigator.userAgentData.mobile)) return 'other';
    var platform = /mac/i.test(hint || '') || /Macintosh|Mac OS X/.test(ua) ? 'macos'
      : /win/i.test(hint || '') || /Windows/.test(ua) ? 'windows'
      : 'other';
    if (platform === 'macos' && navigator.maxTouchPoints > 1) platform = 'other'; // iPad reports Mac
    return platform;
  }

  var tabs = Array.prototype.slice.call(document.querySelectorAll('.tab'));
  function selectTab(platform, focus) {
    tabs.forEach(function (tab) {
      var on = tab.dataset.platform === platform;
      tab.setAttribute('aria-selected', String(on));
      tab.tabIndex = on ? 0 : -1;
      document.getElementById(tab.getAttribute('aria-controls')).hidden = !on;
      if (on && focus) tab.focus();
    });
  }
  tabs.forEach(function (tab, index) {
    tab.addEventListener('click', function () { selectTab(tab.dataset.platform); });
    tab.addEventListener('keydown', function (event) {
      var step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
      if (!step) return;
      event.preventDefault();
      selectTab(tabs[(index + step + tabs.length) % tabs.length].dataset.platform, true);
    });
  });

  function recommend(key) {
    var row = document.querySelector('.pkg[data-key="' + key + '"]');
    if (!row) return null;
    document.querySelectorAll('.pkg[data-platform="' + row.dataset.platform + '"]').forEach(function (item) {
      item.classList.toggle('is-recommended', item === row);
    });
    return row;
  }

  function pointCta(row, platform) {
    var cta = document.getElementById('cta');
    if (!cta || !row) return;
    cta.href = row.dataset.href;
    document.getElementById('cta-title').textContent = data.cta[platform] || data.cta.other;
    document.getElementById('cta-sub').textContent = row.dataset.label + ' · ' + row.dataset.size;
  }

  var platform = detect();
  if (platform !== 'other') selectTab(platform);
  if (platform === 'macos') pointCta(recommend('macos-universal'), 'macos');
  if (platform === 'windows') {
    var x64 = recommend('windows-amd64');
    pointCta(x64, 'windows');
    // Only Chromium tells the CPU; elsewhere x64 stays the pick and both rows remain listed.
    if (navigator.userAgentData && navigator.userAgentData.getHighEntropyValues) {
      navigator.userAgentData.getHighEntropyValues(['architecture']).then(function (values) {
        if (values.architecture === 'arm') pointCta(recommend('windows-arm64'), 'windows');
      }).catch(function () {});
    }
  }

  // --- Hero: alternate the tool name. ---
  var reel = document.querySelector('.reel');
  if (reel && !reduceMotion && data.tools.length > 1) {
    var text = reel.querySelector('.reel-text');
    var current = 0;
    setInterval(function () {
      if (document.hidden) return;
      reel.classList.remove('in');
      reel.classList.add('out');
      setTimeout(function () {
        current = (current + 1) % data.tools.length;
        text.textContent = data.tools[current];
        reel.dataset.tool = String(current);
        reel.classList.remove('out');
        reel.classList.add('in');
      }, 260);
    }, 2800);
  }

  // --- Theme: system by default; the toggle pins light or dark. ---
  var shot = document.querySelector('.shot');
  var source = shot && shot.parentElement.querySelector('source');
  function effectiveTheme() {
    return root.dataset.theme || (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
  }
  function syncShot() {
    if (!shot || !source) return;
    if (root.dataset.theme) {
      source.media = 'not all';
      shot.src = root.dataset.theme === 'dark' ? shot.dataset.dark : shot.dataset.light;
    } else {
      source.media = '(prefers-color-scheme: dark)';
      shot.src = shot.dataset.light;
    }
  }
  syncShot();
  document.querySelector('.theme').addEventListener('click', function () {
    var next = effectiveTheme() === 'dark' ? 'light' : 'dark';
    root.dataset.theme = next;
    store('site-theme', next);
    syncShot();
  });

  // --- Language: remember an explicit switch. URLs never force a language redirect. ---
  document.querySelectorAll('[data-lang]').forEach(function (link) {
    link.addEventListener('click', function () { store('site-lang', link.dataset.lang); });
  });

  // --- Checksums ---
  document.querySelectorAll('.sha').forEach(function (button) {
    button.addEventListener('click', function () {
      var done = function () {
        button.textContent = data.copied;
        button.classList.add('done');
        setTimeout(function () { button.textContent = 'SHA-256'; button.classList.remove('done'); }, 1600);
      };
      if (navigator.clipboard) navigator.clipboard.writeText(button.dataset.sha).then(done, function () {});
    });
  });

  // --- Changelog search ---
  var search = document.getElementById('search');
  if (search) {
    var releases = Array.prototype.slice.call(document.querySelectorAll('.release'));
    var years = Array.prototype.slice.call(document.querySelectorAll('.year'));
    var noMatch = document.querySelector('.no-match');
    search.addEventListener('input', function () {
      var words = search.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
      var shown = 0;
      releases.forEach(function (release) {
        var match = words.every(function (word) { return release.dataset.text.indexOf(word) !== -1; });
        release.hidden = !match;
        if (match) { shown += 1; release.classList.add('in'); }
      });
      years.forEach(function (year) {
        var next = year.nextElementSibling;
        var any = false;
        while (next && next.classList.contains('release')) { if (!next.hidden) any = true; next = next.nextElementSibling; }
        year.hidden = !any;
      });
      noMatch.hidden = shown > 0;
    });
    document.addEventListener('keydown', function (event) {
      var typing = /INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName);
      if (event.key === '/' && !typing) { event.preventDefault(); search.focus(); }
      if (event.key === 'Escape' && document.activeElement === search) { search.value = ''; search.dispatchEvent(new Event('input')); search.blur(); }
    });
  }

  // --- Entry animation and the reading progress bar ---
  var revealed = document.querySelectorAll('.reveal');
  if ('IntersectionObserver' in window && !reduceMotion) {
    var observer = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) { entry.target.classList.add('in'); observer.unobserve(entry.target); }
      });
    }, { rootMargin: '0px 0px -8% 0px' });
    revealed.forEach(function (element) { observer.observe(element); });
  } else {
    revealed.forEach(function (element) { element.classList.add('in'); });
  }

  var progress = document.querySelector('.progress');
  var ticking = false;
  function onScroll() {
    if (ticking) return;
    ticking = true;
    requestAnimationFrame(function () {
      var max = document.documentElement.scrollHeight - window.innerHeight;
      progress.style.setProperty('--scroll', max > 0 ? String(window.scrollY / max) : '0');
      ticking = false;
    });
  }
  window.addEventListener('scroll', onScroll, { passive: true });
  onScroll();
})();
