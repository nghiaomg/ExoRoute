import { ar } from './locales/ar';
import { de } from './locales/de';
import { en } from './locales/en';
import { es } from './locales/es';
import { fr } from './locales/fr';
import { hi } from './locales/hi';
import { id } from './locales/id';
import { ja } from './locales/ja';
import { ko } from './locales/ko';
import { pt } from './locales/pt';
import { vi } from './locales/vi';
import { zh } from './locales/zh';
export { en };

export type Locale = 'en' | 'vi' | 'zh' | 'es' | 'pt' | 'ja' | 'id' | 'ko' | 'de' | 'fr' | 'hi' | 'ar';

export const localeOptions: ReadonlyArray<{ value: Locale; label: string }> = [
  { value: 'en', label: 'English' },
  { value: 'vi', label: 'Tiếng Việt' },
  { value: 'zh', label: '简体中文' },
  { value: 'es', label: 'Español' },
  { value: 'pt', label: 'Português (Brasil)' },
  { value: 'ja', label: '日本語' },
  { value: 'id', label: 'Bahasa Indonesia' },
  { value: 'ko', label: '한국어' },
  { value: 'de', label: 'Deutsch' },
  { value: 'fr', label: 'Français' },
  { value: 'hi', label: 'हिन्दी' },
  { value: 'ar', label: 'العربية' },
];



export type TranslationKey = keyof typeof en;
export type TranslationCatalog = Partial<Record<TranslationKey, string>>;

export const catalogs: Record<Locale, TranslationCatalog> = {
  en,
  vi,
  zh,
  es,
  pt,
  ja,
  id,
  ko,
  de,
  fr,
  hi,
  ar,
};

const intlLocales: Record<Locale, string> = {
  en: 'en-US',
  vi: 'vi-VN',
  zh: 'zh-CN',
  es: 'es-ES',
  pt: 'pt-BR',
  ja: 'ja-JP',
  id: 'id-ID',
  ko: 'ko-KR',
  de: 'de-DE',
  fr: 'fr-FR',
  hi: 'hi-IN',
  ar: 'ar-SA',
};

const LOCALE_STORAGE_KEY = 'exoroute.locale';

export function getIntlLocale(locale: Locale): string {
  return Object.hasOwn(intlLocales, locale) ? intlLocales[locale] : intlLocales.en;
}

function isLocale(value: unknown): value is Locale {
  return typeof value === 'string' && localeOptions.some((option) => option.value === value);
}

export function getStoredLocale(): Locale {
  try {
    if (typeof localStorage !== 'undefined') {
      const saved = localStorage.getItem(LOCALE_STORAGE_KEY);
      if (isLocale(saved)) return saved;
    }
  } catch {
    // Storage can be unavailable in private browsing or restricted contexts.
  }

  return 'en';
}

export function saveLocale(locale: Locale): void {
  if (!isLocale(locale)) return;
  try {
    if (typeof localStorage !== 'undefined') localStorage.setItem(LOCALE_STORAGE_KEY, locale);
  } catch {
    // The active locale still applies for this page even if it cannot be persisted.
  }
}

function lookupTemplate(catalog: TranslationCatalog | undefined, key: string): string | undefined {
  if (!catalog) return undefined;
  if (!Object.hasOwn(catalog, key)) return undefined;
  const value = catalog[key as TranslationKey];
  return typeof value === 'string' ? value : undefined;
}

export function t(locale: Locale, key: string, vars?: Record<string, string | number>): string {
  if (typeof key !== 'string') return '';
  const catalog = Object.hasOwn(catalogs, locale) ? catalogs[locale] : undefined;
  const template = lookupTemplate(catalog, key) ?? lookupTemplate(en, key) ?? key;
  return template.replace(/\{(\w+)\}/g, (placeholder: string, name: string) =>
    vars && Object.hasOwn(vars, name) ? String(vars[name]) : placeholder,
  );
}
