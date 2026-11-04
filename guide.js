/* Presentation only: no mail/account API, analytics or external requests. */
(() => {
  const root = document.documentElement;
  const themeButton = document.querySelector('[data-theme-toggle]');
  const applyTheme = (dark) => {
    root.dataset.theme = dark ? 'dark' : 'light';
    themeButton?.setAttribute('aria-pressed', String(dark));
    themeButton?.setAttribute('aria-label', dark ? '밝은 테마로 전환' : '어두운 테마로 전환');
    if (themeButton) themeButton.textContent = dark ? '☀' : '◐';
  };
  try { applyTheme(localStorage.getItem('hantong-guide-theme') === 'dark'); }
  catch { applyTheme(false); }
  themeButton?.addEventListener('click', () => {
    const dark = root.dataset.theme !== 'dark';
    applyTheme(dark);
    try { localStorage.setItem('hantong-guide-theme', dark ? 'dark' : 'light'); }
    catch { /* A blocked display preference must not break the guide. */ }
  });

  const panel = document.querySelector('.toc-panel');
  const desktop = matchMedia('(min-width: 1000px)');
  if (panel) {
    panel.open = desktop.matches;
    desktop.addEventListener('change', () => { panel.open = desktop.matches; });
  }
  const search = document.querySelector('#toc-search');
  const links = [...document.querySelectorAll('.toc-nav a')];
  const empty = document.querySelector('#toc-empty');
  const filter = () => {
    const value = search.value.trim().toLocaleLowerCase();
    for (const link of links) link.hidden = !link.textContent.toLocaleLowerCase().includes(value);
    for (const group of document.querySelectorAll('.toc-group')) {
      group.hidden = ![...group.querySelectorAll('a')].some(link => !link.hidden);
    }
    empty.hidden = links.some(link => !link.hidden);
  };
  search?.addEventListener('input', filter);
  document.addEventListener('keydown', (event) => {
    if (!search || event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.key === 'Escape' && event.target === search) {
      search.value = ''; filter(); search.blur(); return;
    }
    const typing = event.target.closest?.('input, textarea, [contenteditable="true"]');
    if (event.key === '/' && !typing) {
      event.preventDefault(); if (panel) panel.open = true; search.focus();
    }
  });
  for (const link of links) link.addEventListener('click', () => {
    if (panel && !desktop.matches) panel.open = false;
  });
  if ('IntersectionObserver' in window) {
    const observer = new IntersectionObserver((entries) => {
      const visible = entries.find(entry => entry.isIntersecting);
      if (!visible) return;
      for (const link of links) {
        if (link.hash === '#' + visible.target.id) link.setAttribute('aria-current', 'location');
        else link.removeAttribute('aria-current');
      }
    }, { rootMargin: '-12% 0px -70% 0px' });
    for (const section of document.querySelectorAll('.docs-content section[id]')) observer.observe(section);
  }
  for (const button of document.querySelectorAll('[data-copy-target]')) {
    button.addEventListener('click', async () => {
      const target = document.getElementById(button.dataset.copyTarget);
      if (!target) return;
      try {
        await navigator.clipboard.writeText(target.textContent.trim());
        button.textContent = '복사됨';
      } catch { button.textContent = '텍스트를 선택해 복사'; }
      setTimeout(() => { button.textContent = '복사'; }, 1800);
    });
  }
})();
