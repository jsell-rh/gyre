/**
 * Lightweight markdown-to-HTML renderer for spec content.
 * Handles: headers, bold, italic, code, code blocks, links, lists, blockquotes, HR.
 * No external dependencies.
 */

function escapeHtml(str) {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function inlineMarkdown(line) {
  return line
    // Code spans (must come before bold/italic)
    .replace(/`([^`]+)`/g, '<code class="md-code">$1</code>')
    // Bold + italic
    .replace(/\*\*\*(.+?)\*\*\*/g, '<strong><em>$1</em></strong>')
    // Bold
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    // Italic
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    // Links — sanitize href to prevent javascript: XSS (including entity-encoded bypasses)
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, text, url) => {
      // Decode HTML entities and percent-encoding to catch bypass attempts
      // like &#x6a;avascript: or java%73cript:
      let decoded = url;
      try {
        decoded = decodeURIComponent(url.replace(/&amp;/g, '&'));
      } catch (e) { /* invalid encoding, use raw */ }
      decoded = decoded.replace(/&#x([0-9a-fA-F]+);/g, (_, hex) => String.fromCharCode(parseInt(hex, 16)));
      decoded = decoded.replace(/&#(\d+);/g, (_, dec) => String.fromCharCode(parseInt(dec, 10)));
      const trimmed = decoded.trim().toLowerCase().replace(/[\s\x00-\x1f]+/g, '');
      if (trimmed.startsWith('javascript:') || trimmed.startsWith('data:') || trimmed.startsWith('vbscript:')) {
        return text; // Strip dangerous links, show text only
      }
      // Whitelist safe protocols
      if (!/^(https?:|mailto:|#|\/|\.)/.test(trimmed) && trimmed.includes(':')) {
        return text; // Unknown protocol — strip
      }
      return `<a href="${url}" target="_blank" rel="noopener">${text}</a>`;
    })
    // Spec paths — make clickable with data attribute for event delegation
    .replace(/\b(specs\/[\w\-\/]+\.md)\b/g, '<a href="#" class="md-spec-link" data-spec-path="$1" title="Open $1 in spec editor">$1</a>');
}

export function renderMarkdown(md) {
  if (!md) return '';
  const lines = md.split('\n');
  const out = [];
  let inCodeBlock = false;
  let codeBlockLang = '';
  let codeLines = [];
  let inList = false;
  let listType = 'ul';

  function closeList() {
    if (inList) {
      out.push(`</${listType}>`);
      inList = false;
    }
  }

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Fenced code blocks
    if (line.startsWith('```')) {
      if (inCodeBlock) {
        out.push(`<pre class="md-codeblock"><code>${escapeHtml(codeLines.join('\n'))}</code></pre>`);
        codeLines = [];
        inCodeBlock = false;
      } else {
        closeList();
        inCodeBlock = true;
        codeBlockLang = line.slice(3).trim();
      }
      continue;
    }
    if (inCodeBlock) {
      codeLines.push(line);
      continue;
    }

    // Blank line
    if (line.trim() === '') {
      closeList();
      continue;
    }

    // Headers
    const hMatch = line.match(/^(#{1,6})\s+(.+)/);
    if (hMatch) {
      closeList();
      const level = hMatch[1].length;
      out.push(`<h${level} class="md-h${level}">${inlineMarkdown(escapeHtml(hMatch[2]))}</h${level}>`);
      continue;
    }

    // Horizontal rule
    if (/^(-{3,}|_{3,}|\*{3,})$/.test(line.trim())) {
      closeList();
      out.push('<hr class="md-hr"/>');
      continue;
    }

    // Blockquote
    if (line.startsWith('>')) {
      closeList();
      const content = line.replace(/^>\s?/, '');
      out.push(`<blockquote class="md-blockquote">${inlineMarkdown(escapeHtml(content))}</blockquote>`);
      continue;
    }

    // Unordered list
    const ulMatch = line.match(/^(\s*)[-*+]\s+(.+)/);
    if (ulMatch) {
      if (!inList || listType !== 'ul') {
        closeList();
        out.push('<ul class="md-list">');
        inList = true;
        listType = 'ul';
      }
      out.push(`<li>${inlineMarkdown(escapeHtml(ulMatch[2]))}</li>`);
      continue;
    }

    // Ordered list
    const olMatch = line.match(/^(\s*)\d+\.\s+(.+)/);
    if (olMatch) {
      if (!inList || listType !== 'ol') {
        closeList();
        out.push('<ol class="md-list">');
        inList = true;
        listType = 'ol';
      }
      out.push(`<li>${inlineMarkdown(escapeHtml(olMatch[2]))}</li>`);
      continue;
    }

    // Regular paragraph
    closeList();
    out.push(`<p class="md-p">${inlineMarkdown(escapeHtml(line))}</p>`);
  }

  // Close any open blocks
  if (inCodeBlock) {
    out.push(`<pre class="md-codeblock"><code>${escapeHtml(codeLines.join('\n'))}</code></pre>`);
  }
  closeList();

  return out.join('\n');
}
