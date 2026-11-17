/**
 * Repo tab strip — the single source of truth for the repo-mode tab set.
 *
 * ui-navigation.md §1a: "The tab strip is the source of truth for the item
 * set and the rail mirrors it exactly." RepoMode renders its tab strip from
 * this list, and App.svelte derives the repo-mode navigation rail, the
 * narrow-viewport drawer item set, and the repo-tab URL parsing from the
 * same list. Never hand-maintain a copy of these ids anywhere: a drifted
 * copy is exactly the failure §1a forbids (the App.svelte route-parse list
 * once drifted and silently lost 'dependencies', misparsing deep links to
 * the Specs tab).
 */

export const REPO_TABS = [
  { id: 'specs',        labelKey: 'repo_mode.tabs.specs' },
  { id: 'tasks',        labelKey: 'repo_mode.tabs.tasks' },
  { id: 'mrs',          labelKey: 'repo_mode.tabs.mrs' },
  { id: 'agents',       labelKey: 'repo_mode.tabs.agents' },
  { id: 'architecture', labelKey: 'repo_mode.tabs.architecture' },
  { id: 'dependencies', labelKey: 'repo_mode.tabs.dependencies' },
  { id: 'decisions',    labelKey: 'repo_mode.tabs.decisions' },
  { id: 'code',         labelKey: 'repo_mode.tabs.code' },
  { id: 'settings',     labelKey: 'repo_mode.tabs.settings', titleKey: 'repo_mode.settings_title' },
];

/** Tab ids in canonical tab order — a derivation, never a copy. */
export const REPO_TAB_IDS = REPO_TABS.map((tab) => tab.id);
