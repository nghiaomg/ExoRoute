import type { ChatConversationMessage } from '../../lib/types';

/* ------------------------------------------------------------------ */
/* Export: Markdown and JSON                                           */
/* ------------------------------------------------------------------ */

function safeFileStamp(): string {
  return new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
}

/** Renders the thread as a portable Markdown document. */
export function conversationToMarkdown(
  conversation: ChatConversationMessage[],
  title: string,
): string {
  const lines: string[] = [`# ${title}`, ''];
  for (const message of conversation) {
    if (message.error) {
      lines.push(`> **${'Error'}:** ${message.error}`, '');
      continue;
    }
    const who = message.role === 'user' ? 'You' : message.model || 'Assistant';
    lines.push(`## ${who}`, '', message.text, '');
    if (message.reasoning) {
      lines.push('<details><summary>Reasoning</summary>', '', message.reasoning, '', '</details>', '');
    }
  }
  return lines.join('\n');
}

/** Renders the thread as the full-fidelity JSON archive. */
export function conversationToJson(conversation: ChatConversationMessage[]): string {
  return JSON.stringify(
    {
      exported_at: new Date().toISOString(),
      messages: conversation,
    },
    null,
    2,
  );
}

/** Triggers a bounded client-side download of a text attachment. */
export function downloadTextFile(filename: string, content: string, mime: string): void {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1_000);
}

export function conversationMarkdownFilename(): string {
  return `exoroute-chat-${safeFileStamp()}.md`;
}

export function conversationJsonFilename(): string {
  return `exoroute-chat-${safeFileStamp()}.json`;
}

/** Opens the browser print dialog scoped to the conversation via CSS. */
export function printConversation(): void {
  window.print();
}
