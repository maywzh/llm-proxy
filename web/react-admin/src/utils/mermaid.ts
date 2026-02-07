import type mermaidAPI from 'mermaid';

let mermaidInstance: typeof mermaidAPI | null = null;

async function getMermaid() {
  if (!mermaidInstance) {
    const m = await import('mermaid');
    mermaidInstance = m.default;
    mermaidInstance.initialize({
      startOnLoad: false,
      theme: document.documentElement.classList.contains('dark')
        ? 'dark'
        : 'default',
      securityLevel: 'strict',
      fontFamily: 'inherit',
    });
  }
  return mermaidInstance;
}

let renderCounter = 0;
const svgCache = new Map<string, string>();

export async function renderMermaidBlocks(container: HTMLElement | null) {
  if (!container) return;
  const blocks = container.querySelectorAll<HTMLElement>(
    '.mermaid-block:not([data-mermaid-rendered])'
  );
  if (blocks.length === 0) return;

  const mermaid = await getMermaid();

  for (const block of blocks) {
    const code = block.getAttribute('data-mermaid');
    if (!code) continue;

    const cached = svgCache.get(code);
    if (cached) {
      block.innerHTML = cached;
      block.setAttribute('data-mermaid-rendered', 'true');
      continue;
    }

    const id = `mermaid-${Date.now()}-${renderCounter++}`;
    try {
      const { svg } = await mermaid.render(id, code);
      svgCache.set(code, svg);
      block.innerHTML = svg;
      block.setAttribute('data-mermaid-rendered', 'true');
    } catch {
      document.getElementById('d' + id)?.remove();
      block.setAttribute('data-mermaid-rendered', 'true');
    }
  }
}
