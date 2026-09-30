const all = <T extends Element = HTMLElement>(selector: string) => Array.from(document.querySelectorAll<T>(selector));
const get = <T extends Element = HTMLElement>(selector: string) => document.querySelector<T>(selector);
const read = (key: string) => { try { return localStorage.getItem(key); } catch { return null; } };
const save = (key: string, value: string) => { try { localStorage.setItem(key, value); } catch { /* Preferences are optional. */ } };
const theme = get<HTMLSelectElement>('#theme')!;
theme.value = read('skuld-theme') || 'system';
const applyTheme = () => { document.documentElement.dataset.theme = theme.value === 'system' ? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light') : theme.value; };
theme.addEventListener('change', () => { save('skuld-theme', theme.value); applyTheme(); });
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', applyTheme);
all<HTMLDetailsElement>('.nav-group').forEach(group => {
  const key = `skuld-group-${group.dataset.group}`;
  if (!group.querySelector('[aria-current]') && read(key) === 'closed') group.open = false;
  group.addEventListener('toggle', () => save(key, group.open ? 'open' : 'closed'));
});
all<HTMLButtonElement>('.copy').forEach(button => button.addEventListener('click', async () => {
  const value = button.closest('figure')!.querySelector<HTMLTextAreaElement>('.copy-source')!.value;
  try { await navigator.clipboard.writeText(value); button.textContent = 'Copied'; get('#toast')!.textContent = 'Code copied to clipboard.'; }
  catch { button.textContent = 'Select code'; const range = document.createRange(); range.selectNodeContents(button.closest('figure')!.querySelector('code')!); const selection = window.getSelection(); selection?.removeAllRanges(); selection?.addRange(range); get('#toast')!.textContent = 'Clipboard unavailable. Code selected; use your copy shortcut.'; }
  setTimeout(() => { button.textContent = 'Copy'; }, 1800);
}));
all<HTMLDialogElement>('dialog').forEach(dialog => {
  dialog.querySelector('.close-dialog')?.addEventListener('click', () => dialog.close());
  dialog.addEventListener('click', event => { if (event.target === dialog) { const box = dialog.getBoundingClientRect(); if (event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom) dialog.close(); } });
});
const mobile = get<HTMLDialogElement>('#mobile-navigation')!;
const mobileButton = get<HTMLButtonElement>('.mobile-toggle')!;
mobileButton.addEventListener('click', () => { mobile.showModal(); mobileButton.setAttribute('aria-expanded', 'true'); });
mobile.addEventListener('close', () => mobileButton.setAttribute('aria-expanded', 'false'));
interface SearchPage { title: string; group: string; path: string; description: string; text: string; headings: { title: string; id: string }[] }
let index: SearchPage[] | undefined;
let indexRequest: Promise<SearchPage[]> | undefined;
const searchDialog = get<HTMLDialogElement>('#search-dialog')!;
const input = get<HTMLInputElement>('#search-input')!;
const results = get('#search-results')!;
const status = get('#search-status')!;
let selected = 0;
function search() {
  if (!index) return;
  const terms = input.value.toLowerCase().trim().split(/\s+/).filter(Boolean);
  const found = index.map(page => {
    const text = `${page.title} ${page.description} ${page.text}`.toLowerCase();
    const score = terms.every(t => text.includes(t)) ? terms.reduce((s, t) => s + (page.title.toLowerCase().includes(t) ? 20 : 1) + (page.headings.some(h => h.title.toLowerCase().includes(t)) ? 8 : 0), 0) : -1;
    return { page, score };
  }).filter(r => r.score >= 0).sort((a, b) => b.score - a.score).slice(0, 12);
  results.replaceChildren(); selected = 0;
  for (const { page } of found) {
    const a = document.createElement('a'); a.className = 'search-result';
    const heading = terms.length ? page.headings.find(h => terms.every(t => h.title.toLowerCase().includes(t))) : undefined;
    a.href = page.path + (heading && !terms.every(t => page.title.toLowerCase().includes(t)) ? `#${heading.id}` : '');
    const label = document.createElement('span'); label.textContent = heading ? `${page.title} · ${heading.title}` : page.title;
    const detail = document.createElement('small'); detail.textContent = `${page.group} / ${page.title}`;
    a.append(label, detail); results.append(a);
  }
  status.textContent = found.length ? terms.length ? `${found.length} results for “${input.value}”` : 'Start with a guide, or search for a keyword.' : `No results for “${input.value}”. Try “functions”, “strings” or “install”.`;
  markSelected();
}
function markSelected() { all<HTMLAnchorElement>('.search-result').forEach((a, i) => { a.classList.toggle('selected', i === selected); if (i === selected) a.scrollIntoView({ block: 'nearest' }); }); }
async function openSearch() {
  if (!searchDialog.open) searchDialog.showModal(); input.focus();
  if (!index) {
    status.textContent = 'Loading documentation index…';
    try { indexRequest ??= fetch('/search-index.json').then(r => { if (!r.ok) throw new Error('Unavailable'); return r.json() as Promise<SearchPage[]>; }); index = await indexRequest; }
    catch { indexRequest = undefined; status.textContent = 'Search could not load. Close and reopen to retry, or browse the documentation sidebar.'; return; }
  }
  search();
}
all('[data-search]').forEach(button => button.addEventListener('click', openSearch));
input.addEventListener('input', search);
searchDialog.addEventListener('keydown', event => {
  const links = all<HTMLAnchorElement>('.search-result');
  if (['ArrowDown', 'ArrowUp'].includes(event.key) && links.length) { event.preventDefault(); selected = (selected + (event.key === 'ArrowDown' ? 1 : -1) + links.length) % links.length; markSelected(); }
  if (event.key === 'Enter' && document.activeElement === input && links[selected]) { event.preventDefault(); links[selected].click(); }
});
document.addEventListener('keydown', event => { if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); if (mobile.open) mobile.close(); void openSearch(); } });
const headings = all<HTMLElement>('.prose h2[id],.prose h3[id]');
if (headings.length && 'IntersectionObserver' in window) {
  const updateToc = () => { const active = [...headings].reverse().find(h => h.getBoundingClientRect().top <= 155) || headings[0]; all<HTMLAnchorElement>('.toc-link').forEach(a => { const current = a.hash === `#${active.id}`; a.classList.toggle('active', current); if (current) a.setAttribute('aria-current', 'location'); else a.removeAttribute('aria-current'); }); };
  const observer = new IntersectionObserver(updateToc, { rootMargin: '-90px 0px -65% 0px' }); headings.forEach(h => observer.observe(h)); window.addEventListener('scroll', updateToc, { passive: true }); updateToc();
}
all('[role=tablist]').forEach(tablist => {
  const tabs = Array.from(tablist.querySelectorAll<HTMLButtonElement>('[role=tab]'));
  const select = (tab: HTMLButtonElement) => { tabs.forEach(t => { const active = t === tab; t.setAttribute('aria-selected', String(active)); t.tabIndex = active ? 0 : -1; document.getElementById(t.getAttribute('aria-controls')!)!.hidden = !active; }); };
  tabs.forEach((tab, i) => { tab.addEventListener('click', () => select(tab)); tab.addEventListener('keydown', event => { let next = i; if (event.key === 'ArrowRight') next = (i + 1) % tabs.length; else if (event.key === 'ArrowLeft') next = (i - 1 + tabs.length) % tabs.length; else if (event.key === 'Home') next = 0; else if (event.key === 'End') next = tabs.length - 1; else return; event.preventDefault(); select(tabs[next]); tabs[next].focus(); }); });
});
all<HTMLButtonElement>('[data-filter]').forEach(button => button.addEventListener('click', () => { all('[data-filter]').forEach(b => b.setAttribute('aria-pressed', String(b === button))); all('[data-category]').forEach(card => { card.hidden = button.dataset.filter !== 'all' && card.dataset.category !== button.dataset.filter; }); }));
const editor = get<HTMLTextAreaElement>('#editor');
if (editor) {
  const initial = editor.value; const output = get('#playground-output')!;
  try { const encoded = new URL(location.href).searchParams.get('example'); if (encoded) editor.value = encoded; } catch { /* Keep the initial program. */ }
  const run = async () => { const button = get<HTMLButtonElement>('#run')!; button.disabled = true; button.textContent = 'Checking availability…'; output.setAttribute('aria-busy', 'true'); await Promise.resolve(); output.textContent = 'Playground execution is not available yet.\n\nSave your code as main.skuld and run:\nskuld run main.skuld'; output.setAttribute('aria-busy', 'false'); button.disabled = false; button.textContent = 'Run locally ↗'; };
  get('#run')!.addEventListener('click', run);
  get('#reset')!.addEventListener('click', () => { editor.value = initial; output.textContent = 'Playground execution is not available yet.'; });
  editor.addEventListener('keydown', event => { if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') { event.preventDefault(); void run(); } });
  get('#download')!.addEventListener('click', () => { const url = URL.createObjectURL(new Blob([editor.value], { type: 'text/plain' })); const a = document.createElement('a'); a.href = url; a.download = 'main.skuld'; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); });
}
all<HTMLAnchorElement>('[data-playground]').forEach(a => { const code = a.closest('.example-card')?.querySelector<HTMLTextAreaElement>('.copy-source')?.value; if (code) a.href = `/playground/?example=${encodeURIComponent(code)}`; });
