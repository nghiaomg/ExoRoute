/**
 * A small, self-contained markdown subset renderer for chat replies.
 *
 * Design rules:
 * - Escape-first: the input is HTML-escaped before any transformation, so a
 *   reply can never inject markup (`<script>`, `on*=` handlers, `javascript:`
 *   URLs) through this renderer. Only constructs this file emits itself can
 *   appear in the output.
 * - Supported subset: fenced code blocks with language labels, inline code,
 *   bold/italic, links (http/https/mailto only), unordered/ordered lists,
 *   blockquotes, headings, and paragraphs. Everything else stays literal text.
 * - No dependencies and no DOM writes of unescaped strings.
 */

const CODE_FENCE = /^```(\w*)\s*$/;
const HEADING = /^(#{1,3})\s+(.*)$/;
const UNORDERED = /^\s*[-*+]\s+(.*)$/;
const ORDERED = /^\s*\d+[.)]\s+(.*)$/;
const BLOCKQUOTE = /^\s*>\s?(.*)$/;

/** HTML-escapes every input character that could start markup. */
export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/** Renders inline markdown (code, emphasis, links). Escapes its input first. */
function renderInline(raw: string): string {
  // Pass 0: escape the whole line once. Every later pass may only emit
  // markup this file constructs itself, never raw input text.
  let working = escapeHtml(raw);

  // Pass 1: protect inline code spans so emphasis rules never touch them.
  // The span content is already escaped.
  const codeSpans: string[] = [];
  working = working.replace(/`([^`\n]+)`/g, (_match, code: string) => {
    codeSpans.push(`<code class="md-code">${code}</code>`);
    return `\u0000${codeSpans.length - 1}\u0000`;
  });

  // Pass 2: links. `[label](url)` with safe schemes only; both label and
  // url are already escaped text.
  working = working.replace(
    /\[([^\]\n]+)\]\(([^)\s]+)\)/g,
    (match, label: string, url: string) => {
      const scheme = url.slice(0, url.indexOf(':') > 0 ? url.indexOf(':') : 0).toLowerCase();
      if (!/^(https?|mailto)$/.test(scheme)) return match;
      return `<a class="md-link" href="${url}" target="_blank" rel="noopener noreferrer">${renderEmphasis(label)}</a>`;
    },
  );

  working = renderEmphasis(working);

  // Pass 3: restore protected code spans.
  return working.replace(/\u0000(\d+)\u0000/g, (_match, index: string) => {
    return codeSpans[Number(index)] ?? '';
  });
}

/** Applies bold/italic on already-escaped text. */
function renderEmphasis(text: string): string {
  let out = text.replace(/\*\*([^*\n]+)\*\*/g, '<strong class="md-strong">$1</strong>');
  out = out.replace(/(^|[\s(])\*([^*\n]+)\*(?=$|[\s.,;:!?)])/, '$1<em class="md-em">$2</em>');
  return out;
}

/** Renders one full reply to safe HTML for {@linkcode ChatConversation}. */
export function renderMarkdown(source: string): string {
  const lines = source.split(/\r\n|\r|\n/);
  const blocks: string[] = [];
  let paragraph: string[] = [];
  let listItems: { ordered: boolean; items: string[] } | null = null;
  let quote: string[] = [];

  const flushParagraph = (): void => {
    if (paragraph.length) {
      blocks.push(`<p class="md-p">${renderInline(paragraph.join(' '))}</p>`);
      paragraph = [];
    }
  };
  const flushList = (): void => {
    if (listItems) {
      const tag = listItems.ordered ? 'ol' : 'ul';
      const body = listItems.items.map((item) => `<li>${renderInline(item)}</li>`).join('');
      blocks.push(`<${tag} class="md-list">${body}</${tag}>`);
      listItems = null;
    }
  };
  const flushQuote = (): void => {
    if (quote.length) {
      blocks.push(`<blockquote class="md-quote">${renderInline(quote.join(' '))}</blockquote>`);
      quote = [];
    }
  };
  const flushAll = (): void => {
    flushParagraph();
    flushList();
    flushQuote();
  };

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const fence = CODE_FENCE.exec(line);
    if (fence) {
      flushAll();
      const language = fence[1] ?? '';
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !CODE_FENCE.test(lines[index])) {
        code.push(lines[index]);
        index += 1;
      }
      const languageLabel = language
        ? `<span class="md-code-lang">${escapeHtml(language)}</span>`
        : '';
      blocks.push(
        `<div class="md-code-block">${languageLabel}<pre class="md-pre"><code>${escapeHtml(code.join('\n'))}</code></pre></div>`,
      );
      continue;
    }
    if (!line.trim()) {
      flushAll();
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      flushAll();
      const level = heading[1].length;
      blocks.push(`<h${level} class="md-h md-h${level}">${renderInline(heading[2])}</h${level}>`);
      continue;
    }
    const unordered = UNORDERED.exec(line);
    if (unordered) {
      flushParagraph();
      flushQuote();
      if (!listItems || listItems.ordered) {
        flushList();
        listItems = { ordered: false, items: [] };
      }
      listItems.items.push(unordered[1]);
      continue;
    }
    const ordered = ORDERED.exec(line);
    if (ordered) {
      flushParagraph();
      flushQuote();
      if (!listItems || !listItems.ordered) {
        flushList();
        listItems = { ordered: true, items: [] };
      }
      listItems.items.push(ordered[1]);
      continue;
    }
    const quoteLine = BLOCKQUOTE.exec(line);
    if (quoteLine) {
      flushParagraph();
      flushList();
      quote.push(quoteLine[1]);
      continue;
    }
    flushList();
    flushQuote();
    paragraph.push(line.trim());
  }
  flushAll();
  return blocks.join('\n');
}

/**
 * A short single-line preview for quotes and palette entries: strips markdown
 * markers and collapses whitespace. Bounded by the caller.
 */
export function markdownPreview(text: string, maxChars = 120): string {
  const flat = text
    .replace(/```[\w]*\n?/g, ' ')
    .replace(/`([^`\n]+)`/g, '$1')
    .replace(/!?\[([^\]\n]+)\]\([^)\s]*\)/g, '$1')
    .replace(/[*_>#]+/g, '')
    .replace(/\s+/g, ' ')
    .trim();
  if (flat.length <= maxChars) return flat;
  return `${flat.slice(0, Math.max(0, maxChars - 1)).trimEnd()}…`;
}

/** Counts fenced code blocks in a reply, for the copy-code affordance. */
export function countCodeBlocks(text: string): number {
  return Math.floor((text.match(/```/g)?.length ?? 0) / 2);
}
