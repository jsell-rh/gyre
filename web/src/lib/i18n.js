import { addMessages, init, getLocaleFromNavigator } from 'svelte-i18n';
import en from '../locales/en.json';

addMessages('en', en);

init({
  fallbackLocale: 'en',
  initialLocale: getLocaleFromNavigator() || 'en',
});
