export * from './core';

import { authApi } from './auth';
import { comboApi } from './combos';
import { overviewApi } from './overview';
import { providerApi } from './providers';
import { quotaApi } from './quota';
import { settingsApi } from './settings';
import { apiKeyApi } from './api-keys';
import { modelTestApi } from './model-tests';
import { requestApi } from './requests';
import { routeApi } from './routes';
import { statisticsApi } from './statistics';

/**
 * Stable dashboard API surface composed from feature-scoped endpoint modules.
 * Consumers keep using api.<method>; transport/auth policy lives in api/core.
 */
export const api = {
  ...authApi,
  ...overviewApi,
  ...providerApi,
  ...quotaApi,
  ...comboApi,
  ...apiKeyApi,
  ...modelTestApi,
  ...requestApi,
  ...routeApi,
  ...settingsApi,
  ...statisticsApi,
};
