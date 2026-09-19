import { ApiError } from './api';
import { en } from './locales/en';
import type { Translate } from './format';

export function localizedError(error: unknown, fallback: string, tr: Translate): string {
  if (!(error instanceof ApiError)) return tr(fallback);
  const providerModelsHttpFailure = error.message.match(/^provider returned HTTP (\d+) while importing models$/);
  if (providerModelsHttpFailure) {
    return tr('provider returned HTTP {status} while importing models', { status: providerModelsHttpFailure[1] });
  }
  if (!Object.hasOwn(en, error.message)) return error.message;
  return tr(error.message);
}
