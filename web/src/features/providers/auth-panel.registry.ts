import CodexAccountsPanel from './CodexAccountsPanel.svelte';
import CodexOAuthPanel from './CodexOAuthPanel.svelte';
import CommandCodeAuthAssistPanel from './CommandCodeAuthAssistPanel.svelte';
import DeviceCodeOAuthPanel from './DeviceCodeOAuthPanel.svelte';

export interface ProviderAuthPanels {
  oauth: typeof CodexOAuthPanel | null;
  accounts: typeof CodexAccountsPanel | null;
  apiKey: typeof CommandCodeAuthAssistPanel | null;
}

export const KNOWN_AUTH_PANELS = ['openai_codex', 'command_code', 'cline', 'kilocode'] as const;
export type KnownAuthPanel = (typeof KNOWN_AUTH_PANELS)[number];

export function resolveAuthPanel(panel: string | null | undefined): ProviderAuthPanels | null {
  if (!panel) return null;
  if (!(KNOWN_AUTH_PANELS as readonly string[]).includes(panel)) return null;
  const key = panel as KnownAuthPanel;
  return Object.hasOwn(providerAuthPanelRegistry, key) ? providerAuthPanelRegistry[key] : null;
}

export const providerAuthPanelRegistry: Record<KnownAuthPanel, ProviderAuthPanels> = {
  openai_codex: {
    oauth: CodexOAuthPanel,
    accounts: CodexAccountsPanel,
    apiKey: null,
  },
  command_code: {
    oauth: null,
    accounts: null,
    apiKey: CommandCodeAuthAssistPanel,
  },
  cline: {
    oauth: CodexOAuthPanel,
    accounts: CodexAccountsPanel,
    apiKey: null,
  },
  kilocode: {
    oauth: DeviceCodeOAuthPanel,
    accounts: CodexAccountsPanel,
    apiKey: null,
  },
};
