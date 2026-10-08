/**
 * Vendor identity of a model id.
 *
 * `assets/models` names each logo after its vendor (`openai.svg`, `z-ai.svg`),
 * while a model id names a family (`gpt-5.4`, `glm-5.3`, `ocg/minimax-m3`).
 * This module maps the second onto the first without knowing which files exist,
 * so dropping a new `<vendor>.svg` into that folder is enough for its own model
 * ids to resolve — the file set, not this table, decides what can be drawn.
 *
 * Kept free of asset imports so the mapping stays testable under plain Node.
 */

/**
 * Vendor families whose model ids do not contain the file's own name. Order is
 * the tie-break order when one model id matches two vendors.
 */
const VENDOR_ALIASES: ReadonlyArray<readonly [stem: string, tokens: readonly string[]]> = [
  ['openai', ['gpt', 'o1', 'o3', 'o4', 'codex', 'chatgpt', 'davinci']],
  ['claude', ['claude', 'anthropic']],
  ['gemini', ['gemini', 'google', 'imagen', 'palm']],
  ['grok', ['grok', 'xai']],
  ['deepseek', ['deepseek']],
  ['kimi', ['kimi', 'moonshot']],
  ['minimax', ['minimax', 'abab']],
  ['xiaomi', ['mimo', 'xiaomi']],
  ['z-ai', ['glm', 'zai', 'zhipu']],
  ['qwen', ['qwen', 'qwq']],
  ['meta', ['llama', 'meta']],
  ['mistral', ['mistral', 'mixtral', 'codestral', 'magistral', 'devstral', 'pixtral']],
  ['tencenthy', ['hy', 'hunyuan', 'tencent']],
  ['poolside', ['poolside', 'laguna']],
  ['sakana', ['sakana', 'fugu']],
  ['inclusion', ['ling', 'ring']],
];

function modelTokens(model: string): string[] {
  return model
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((token) => token.length > 1 && /[a-z]/.test(token));
}

function matchesAlias(token: string, alias: string): boolean {
  if (token === alias) return true;
  if (!token.startsWith(alias)) return false;
  // `qwen3.8-max` and `hy4-preview` glue the generation onto the vendor name.
  const suffix = token.slice(alias.length);
  return suffix.length > 0 && /^\d/.test(suffix);
}

function aliasStem(token: string): string | null {
  for (const [stem, aliases] of VENDOR_ALIASES) {
    if (aliases.some((alias) => matchesAlias(token, alias))) return stem;
  }
  return null;
}

/**
 * The vendor logo file stem for `model`, or `null` when no available file
 * matches. Tokens are read left to right, so the vendor segment of an id such as
 * `openai/gpt-5` or `anthropic/claude-opus-4-6` decides before the family does;
 * a token that names no known family is offered to `available` unchanged, which
 * is what lets a newly added `<vendor>.svg` cover its own model ids.
 */
export function pickModelVendor(model: string | null | undefined, available: ReadonlySet<string>): string | null {
  if (!model) return null;
  for (const token of modelTokens(model)) {
    const stem = aliasStem(token) ?? token;
    if (available.has(stem)) return stem;
  }
  return null;
}
