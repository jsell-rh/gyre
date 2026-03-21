import '@testing-library/jest-dom';
import { addMessages, init } from 'svelte-i18n';
import en from '../locales/en.json';

// Initialize svelte-i18n for tests
addMessages('en', en);
init({ fallbackLocale: 'en', initialLocale: 'en' });

// Mock fetch globally for all tests
global.fetch = vi.fn(() =>
  Promise.resolve({
    ok: true,
    status: 200,
    statusText: 'OK',
    json: () => Promise.resolve([]),
  })
);

// Reset mocks between tests
beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  global.fetch = vi.fn(() =>
    Promise.resolve({
      ok: true,
      status: 200,
      statusText: 'OK',
      json: () => Promise.resolve([]),
    })
  );
});
