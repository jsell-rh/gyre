import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

// $lib alias — the path convention shadcn-svelte components are generated
// against (components.json aliases). Resolves to web/src/lib.
const libDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), 'src/lib');

// Lint plugin: catch Svelte 5 runes ($state, $derived, etc.) in plain .js/.ts
// files that won't be processed by the Svelte compiler.
function svelteRuneLint() {
  const RUNE_RE = /\$(?:state|derived|effect|props|bindable|inspect)\s*[\(\.\[]/;
  return {
    name: 'svelte-rune-lint',
    transform(code, id) {
      if (id.includes('node_modules')) return null;
      if (id.endsWith('.svelte') || id.includes('.svelte.')) return null;
      if (RUNE_RE.test(code)) {
        this.error(
          `Svelte 5 rune used in plain JS file: ${id}\n` +
          'Rename to .svelte.js so the Svelte compiler processes it.'
        );
      }
      return null;
    },
  };
}

export default defineConfig({
  plugins: [tailwindcss(), svelte(), svelteRuneLint()],
  resolve: {
    alias: {
      '$lib': libDir,
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
 server: {
    proxy: {
      '/api': 'http://localhost:3000',
      '/mcp': 'http://localhost:3000',
      '/git': 'http://localhost:3000',
      '/ws': { target: 'http://localhost:3000', ws: true },
    }
  }
});
