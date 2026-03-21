import { addMessages, init } from 'svelte-i18n';
import en from '../locales/en.json';

addMessages('en', en);

// Use 'en' directly — avoids async locale lookup when navigator returns 'en-US'
// (not registered), which triggers a Promise-based resolution race on first render.
init({
  fallbackLocale: 'en',
  initialLocale: 'en',
});
