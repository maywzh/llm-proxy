import DOMPurify from 'isomorphic-dompurify';
import { marked, type Tokens } from 'marked';
import hljs from 'highlight.js';

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

marked.use({
  gfm: true,
  breaks: true,
  renderer: {
    code({ text, lang }: Tokens.Code) {
      const rawLang = (lang ?? '').trim();
      const normalizedLang = rawLang.split(/\s+/)[0].toLowerCase();

      if (normalizedLang === 'mermaid') {
        return `<div class="mermaid-block" data-mermaid="${escapeHtml(text)}">${escapeHtml(text)}</div>`;
      }

      const language =
        normalizedLang && hljs.getLanguage(normalizedLang)
          ? normalizedLang
          : undefined;

      const highlighted = language
        ? hljs.highlight(text, { language }).value
        : hljs.highlightAuto(text).value;

      const langClass = language
        ? `language-${language}`
        : 'language-plaintext';
      return `<pre><code class="hljs ${langClass}">${highlighted}</code></pre>`;
    },
  },
});

export function renderMarkdownToHtml(markdown: string): string {
  const html = marked.parse(markdown ?? '') as string;
  return DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    ADD_ATTR: ['data-mermaid'],
  });
}
