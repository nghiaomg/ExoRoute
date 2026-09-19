import type { Translate } from './format';

export function labelProtocol(value: string, tr: Translate): string {
  return ({
    chat_completions: tr('Chat Completions'),
    responses: tr('Responses'),
    messages: tr('Messages'),
    google_generate_content: tr('Google Generate Content'),
  } as Record<string, string>)[value] ?? value.replaceAll('_', ' ');
}

export function protocolBadgeClass(value?: string | null): string {
  if (!value) return '';
  const p = value.toLowerCase().trim();
  if (p === 'chat_completions' || p === 'chat') {
    return 'protocol-chat';
  }
  if (p === 'messages' || p === 'anthropic') {
    return 'protocol-messages';
  }
  if (p === 'responses') {
    return 'protocol-responses';
  }
  return '';
}

export function labelAuthType(value: string, tr: Translate): string {
  return ({
    none: tr('None'),
    bearer: tr('Bearer token'),
    header: tr('Custom header'),
    codex_oauth: tr('OpenAI Codex OAuth'),
    antigravity_oauth: tr('Antigravity OAuth'),
  } as Record<string, string>)[value] ?? value;
}

export function labelStrategy(value: string, tr: Translate): string {
  return ({ priority: tr('Ordered fallback'), round_robin: tr('Round robin fallback') } as Record<string, string>)[value] ?? value.replaceAll('_', ' ');
}
