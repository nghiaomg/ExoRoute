import { pickModelVendor } from './model-vendor';

// Every SVG in `assets/models`, keyed by its path. The folder is the source of
// truth: adding `<vendor>.svg` publishes that vendor here with no code change,
// which is why the mapping in `model-vendor` is consulted against these stems
// rather than against a hard-coded vendor list.
const modelAssets = import.meta.glob('../../../../assets/models/*.svg', {
  eager: true,
  query: '?url',
  import: 'default',
});

function logoByStem(): Record<string, string> {
  const logos: Record<string, string> = {};
  for (const [path, url] of Object.entries(modelAssets)) {
    if (typeof url !== 'string') continue;
    const stem = path.split('/').pop()?.replace(/\.svg$/, '');
    if (stem && !logos[stem]) logos[stem] = url;
  }
  return logos;
}

const modelLogos = logoByStem();
const modelVendors = new Set(Object.keys(modelLogos));

/** Bundled logo URL for a model id, or `null` when no vendor logo matches. */
export function modelLogoSrc(model: string | null | undefined): string | null {
  const vendor = pickModelVendor(model, modelVendors);
  return vendor ? (modelLogos[vendor] ?? null) : null;
}
