// Shared DOM helpers: element creation + keyed row updates that preserve
// focus and scroll position. No framework, no virtual DOM — rebuild only
// what changed, keyed by stable task ID.

export function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null) continue;
    if (k === "class") node.className = v;
    else if (k === "text") node.textContent = v;
    else if (k === "html") node.innerHTML = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else node.setAttribute(k, v);
  }
  for (const c of children) {
    if (c == null) continue;
    node.append(c.nodeType ? c : document.createTextNode(c));
  }
  return node;
}

// Update children of `container` to match `items` (each with a stable .key),
// reusing existing DOM nodes keyed by data-key. Preserves focus + scroll.
export function syncKeyed(container, items, makeNode, updateNode) {
  const have = new Map();
  for (const child of [...container.children]) {
    if (child.dataset.key) have.set(child.dataset.key, child);
  }
  const want = new Set(items.map((i) => i.key));
  for (const [key, node] of have) {
    if (!want.has(key)) node.remove();
  }
  let prev = null;
  for (const item of items) {
    let node = have.get(item.key);
    if (!node) {
      node = makeNode(item);
      node.dataset.key = item.key;
    } else if (updateNode) {
      updateNode(node, item);
    }
    const ref = prev ? prev.nextSibling : container.firstChild;
    if (node !== ref) container.insertBefore(node, ref);
    prev = node;
  }
}

// Debounce for search input etc.
export function debounce(fn, ms) {
  let t;
  return (...args) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...args), ms);
  };
}

export function fmtAge(ms) {
  if (ms == null) return "—";
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

export function fmtClock(ms) {
  if (ms == null) return "";
  const d = new Date(ms);
  return d.toTimeString().slice(0, 8);
}

export function badge(text, cls) {
  return el("span", { class: `badge ${cls || ""}`, text });
}
