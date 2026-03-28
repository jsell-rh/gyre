import { describe, it, expect } from 'vitest';
import { detectLang, highlightLine } from '../lib/syntaxHighlight.js';

describe('detectLang', () => {
  it('detects Rust from .rs', () => expect(detectLang('src/main.rs')).toBe('rust'));
  it('detects JS from .js', () => expect(detectLang('src/app.js')).toBe('javascript'));
  it('detects TS from .ts', () => expect(detectLang('lib/api.ts')).toBe('typescript'));
  it('detects Python from .py', () => expect(detectLang('build.py')).toBe('python'));
  it('detects Go from .go', () => expect(detectLang('main.go')).toBe('go'));
  it('detects TOML from .toml', () => expect(detectLang('Cargo.toml')).toBe('toml'));
  it('detects JSON from .json', () => expect(detectLang('package.json')).toBe('json'));
  it('detects YAML from .yaml', () => expect(detectLang('ci.yaml')).toBe('yaml'));
  it('detects YAML from .yml', () => expect(detectLang('docker-compose.yml')).toBe('yaml'));
  it('detects HTML from .html', () => expect(detectLang('index.html')).toBe('html'));
  it('detects HTML from .svelte', () => expect(detectLang('App.svelte')).toBe('html'));
  it('detects CSS from .css', () => expect(detectLang('main.css')).toBe('css'));
  it('detects shell from .sh', () => expect(detectLang('deploy.sh')).toBe('shell'));
  it('detects Nix from .nix', () => expect(detectLang('flake.nix')).toBe('nix'));
  it('detects SQL from .sql', () => expect(detectLang('0001.sql')).toBe('sql'));
  it('detects Makefile by name', () => expect(detectLang('Makefile')).toBe('shell'));
  it('detects Dockerfile by name', () => expect(detectLang('Dockerfile')).toBe('shell'));
  it('returns text for unknown ext', () => expect(detectLang('file.xyz')).toBe('text'));
  it('returns text for no extension', () => expect(detectLang('CHANGELOG')).toBe('text'));
  it('handles paths with multiple dots', () => expect(detectLang('foo.test.ts')).toBe('typescript'));
});

describe('highlightLine — XSS safety', () => {
  it('escapes < > & in plain text', () => {
    const out = highlightLine('<script>&test</script>', 'text');
    expect(out).not.toContain('<script>');
    expect(out).toContain('&lt;script&gt;');
    expect(out).toContain('&amp;');
  });
  it('escapes HTML entities in Rust content', () => {
    const out = highlightLine('let x = a < b && c > d;', 'rust');
    expect(out).toContain('&lt;');
    expect(out).toContain('&amp;&amp;');
    expect(out).toContain('&gt;');
    expect(out).not.toMatch(/<(?!span|\/span)/);
  });
});

describe('highlightLine — Rust', () => {
  it('wraps fn keyword', () => expect(highlightLine('fn main() {','rust')).toContain('<span class="hl-kw">fn</span>'));
  it('wraps string literals', () => expect(highlightLine('let s = "hello";','rust')).toContain('<span class="hl-str">&quot;hello&quot;</span>'));
  it('wraps // comments', () => expect(highlightLine('// comment','rust')).toContain('<span class="hl-cmt">// comment</span>'));
  it('wraps numbers', () => expect(highlightLine('let x = 42;','rust')).toContain('<span class="hl-num">42</span>'));
  it('wraps hex numbers', () => expect(highlightLine('let x = 0xFF;','rust')).toContain('<span class="hl-num">0xFF</span>'));
  it('wraps let mut', () => {
    const out = highlightLine('let mut count = 0;','rust');
    expect(out).toContain('<span class="hl-kw">let</span>');
    expect(out).toContain('<span class="hl-kw">mut</span>');
  });
  it('highlights Option and String', () => {
    const out = highlightLine('pub fn spawn(id: u32) -> Option<String> {','rust');
    expect(out).toContain('<span class="hl-kw">pub</span>');
    expect(out).toContain('<span class="hl-kw">fn</span>');
    expect(out).toContain('<span class="hl-kw">Option</span>');
    expect(out).toContain('<span class="hl-kw">String</span>');
  });
  it('stops at // comment mid-line', () => expect(highlightLine('let x = 1; // comment','rust')).toContain('<span class="hl-cmt">// comment</span>'));
  it('block comment highlighted', () => expect(highlightLine('let x = /* inline */ 5;','rust')).toContain('<span class="hl-cmt">/* inline */</span>'));
});

describe('highlightLine — JavaScript', () => {
  it('wraps const/function', () => {
    const out = highlightLine('const fn = function() {}','javascript');
    expect(out).toContain('<span class="hl-kw">const</span>');
    expect(out).toContain('<span class="hl-kw">function</span>');
  });
  it('wraps template literals', () => expect(highlightLine('const s = `hello`;','javascript')).toContain('hl-str'));
  it('wraps null and undefined', () => {
    const out = highlightLine('let x = null; let y = undefined;','javascript');
    expect(out).toContain('<span class="hl-kw">null</span>');
    expect(out).toContain('<span class="hl-kw">undefined</span>');
  });
});

describe('highlightLine — Python', () => {
  it('highlights def', () => expect(highlightLine('def compute(x):','python')).toContain('<span class="hl-kw">def</span>'));
  it('wraps # comments', () => expect(highlightLine('# python comment','python')).toContain('<span class="hl-cmt"># python comment</span>'));
  it('highlights True False None', () => {
    const out = highlightLine('x = True; y = False; z = None','python');
    expect(out).toContain('<span class="hl-kw">True</span>');
    expect(out).toContain('<span class="hl-kw">False</span>');
    expect(out).toContain('<span class="hl-kw">None</span>');
  });
  it('wraps string literals', () => expect(highlightLine("msg = 'hello world'",'python')).toContain("<span class=\"hl-str\">&#39;hello world&#39;</span>"));
});

describe('highlightLine — Go', () => {
  it('highlights func', () => expect(highlightLine('func main() {','go')).toContain('<span class="hl-kw">func</span>'));
  it('wraps // comments', () => expect(highlightLine('// go comment','go')).toContain('<span class="hl-cmt">// go comment</span>'));
});

describe('highlightLine — JSON', () => {
  it('wraps string values', () => {
    const out = highlightLine('  "name": "gyre"','json');
    expect(out).toContain('<span class="hl-str">&quot;name&quot;</span>');
    expect(out).toContain('<span class="hl-str">&quot;gyre&quot;</span>');
  });
  it('wraps numbers', () => expect(highlightLine('  "port": 3000','json')).toContain('<span class="hl-num">3000</span>'));
  it('wraps true/false/null', () => {
    const out = highlightLine('  "active": true, "data": null','json');
    expect(out).toContain('<span class="hl-kw">true</span>');
    expect(out).toContain('<span class="hl-kw">null</span>');
  });
});

describe('highlightLine — edge cases', () => {
  it('returns plain text for text lang', () => {
    const out = highlightLine('plain text line','text');
    expect(out).toBe('plain text line');
    expect(out).not.toContain('<span');
  });
  it('returns plain text for undefined lang', () => expect(highlightLine('hello world',undefined)).toBe('hello world'));
  it('handles empty string', () => expect(highlightLine('','rust')).toBe(''));
  it('handles whitespace-only', () => expect(highlightLine('    ','rust')).toBe('    '));
  it('handles single char', () => expect(highlightLine('{','rust')).toBe('{'));
});
