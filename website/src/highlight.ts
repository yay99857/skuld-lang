import hljs from 'highlight.js';
hljs.registerLanguage('skuld', () => ({
  name: 'Skuld',
  keywords: {
    keyword: 'func let var const static return if else while loop for in break continue new weak class struct enum match import pub extern unsafe interface defer',
    type: 'int float bool char string void i8 i16 i32 i64 u8 u16 u32 u64 isize usize Option Result',
    literal: 'true false null None',
    built_in: 'print Some Ok Err ptr load store offset addr ptr_from volatile_load volatile_store size_of offset_of bytes_to_string',
  },
  contains: [hljs.C_LINE_COMMENT_MODE, { scope: 'string', begin: '"', end: '"', contains: [hljs.BACKSLASH_ESCAPE, { scope: 'subst', begin: /\$\{/, end: /\}/, keywords: 'this' }] }, { scope: 'string', begin: "'", end: "'", contains: [hljs.BACKSLASH_ESCAPE] }, { scope: 'number', begin: /\b(0[xX][\da-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(\.\d[\d_]*)?)/ }, { scope: 'title.function', begin: /\b[A-Za-z_]\w*(?=\()/ }],
}));
export const escape = (s: string) => s.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&#39;');
export function codeBlock(source: string, info = 'text') {
  const [language, filename] = info.split(' ');
  const numbers = info.includes('numbers');
  const selected = (info.match(/highlight=([\d,]+)/)?.[1] || '').split(',').map(Number);
  const highlighted = hljs.getLanguage(language) ? hljs.highlight(source, { language }).value : escape(source);
  const lines = highlighted.split('\n').map((line, i) => `<span class="code-line${selected.includes(i + 1) ? ' highlighted' : ''}">${numbers ? `<span class="line-number" aria-hidden="true">${i + 1}</span>` : ''}${line || ' '}</span>`).join('');
  return `<figure class="code-block"><figcaption><span>${escape(filename && !filename.includes('=') && filename !== 'numbers' ? filename : language)}</span><button class="copy" type="button" aria-label="Copy ${escape(language)} code">Copy</button></figcaption><pre tabindex="0"><code class="language-${escape(language)}">${lines}</code></pre><textarea class="copy-source" hidden>${escape(source)}</textarea></figure>`;
}
