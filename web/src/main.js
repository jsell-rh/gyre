// app.css is the Tailwind entry stylesheet — it MUST load before the app's
// global stylesheet (design-system.css, imported by App.svelte) so the layer
// order declared in app.css holds. Removing this import unstyles every
// shadcn-svelte ui component (their classes are Tailwind utilities).
import './app.css';
import './lib/i18n.js';
import { mount } from 'svelte';
import App from './App.svelte';

const app = mount(App, { target: document.getElementById('app') });

export default app;
