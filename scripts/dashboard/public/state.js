// URL-backed UI state: view, selection, filters, search, drawer tab.
// Everything navigable lives in the URL (bookmarkable filtered views).
// focusIndex is session-only (NOT in the URL) — kept in a module-level
// variable so successive j/k presses advance instead of resetting to 0.
let focusIndex = 0;

function fromURL() {
  const u = new URL(location.href);
  return {
    view: u.searchParams.get("v") || "overview",
    selectedTask: u.searchParams.get("t") || null,
    drawerTab: u.searchParams.get("tab") || "activity",
    filter: u.searchParams.get("f") || "active+attention",
    search: u.searchParams.get("q") || "",
    focusIndex,
  };
}

export function initialState() {
  return fromURL();
}

export function getState() {
  return fromURL();
}

export function setState(patch) {
  const cur = fromURL();
  const next = { ...cur, ...patch };
  if ("focusIndex" in patch) focusIndex = next.focusIndex;
  const u = new URL(location.href);
  const sp = u.searchParams;
  setParam(sp, "v", next.view, "overview");
  setParam(sp, "t", next.selectedTask, null);
  setParam(sp, "tab", next.drawerTab, "activity");
  setParam(sp, "f", next.filter, "active+attention");
  setParam(sp, "q", next.search, "");
  history.replaceState(null, "", u.href);
  return next;
}

function setParam(sp, key, value, defaultVal) {
  if (value == null || value === "" || value === defaultVal) sp.delete(key);
  else sp.set(key, value);
}
