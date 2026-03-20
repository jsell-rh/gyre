# Milestone 8: Frontend Excellence

Transform Gyre's dashboard from a functional prototype into a delightful, production-quality interface following Red Hat brand standards. Intentional UX design, consistent user journeys, polished visual design.

## Design System: Red Hat Brand Standards

### Color Palette

**Core palette (dark theme):**
- Background: `gray-95` (#151515) — "UX black", primary background
- Surface: `gray-90` (#1f1f1f) — cards, panels, modals
- Surface elevated: `gray-80` (#292929) — hover states, elevated cards
- Border: `gray-70` (#383838) — subtle borders
- Border strong: `gray-60` (#4d4d4d) — prominent borders
- Text primary: `white` (#ffffff)
- Text secondary: `gray-40` (#a3a3a3)
- Text muted: `gray-50` (#707070)

**Accent colors:**
- Primary action: `red-50` (#ee0000) — Red Hat red, primary buttons, active states
- Primary hover: `red-60` (#a60000)
- Link/interactive: `interaction-blue-50` (#0066cc)
- Link hover: `interaction-blue-40` (#4394e5)

**Status colors (information palette only):**
- Success: `success-green-50` (#63993d) — passed, merged, healthy
- Warning: `orange-40` (#f5921b) — in progress, processing
- Danger: `danger-orange-50` (#f0561d) — failed, error, dead
- Info: `interaction-blue-50` (#0066cc) — informational badges
- Blocked: `purple-50` (#5e40be)

**Semantic mapping:**
- Agent Active: `success-green-50`
- Agent Idle: `gray-50`
- Agent Blocked: `purple-50`
- Agent Error/Dead: `danger-orange-50`
- Task Backlog: `gray-50`
- Task InProgress: `orange-40`
- Task Review: `interaction-blue-50`
- Task Done: `success-green-50`
- Task Blocked: `purple-50`
- MR Open: `interaction-blue-50`
- MR Approved: `success-green-50`
- MR Merged: `purple-50`
- MR Closed: `gray-50`
- Queue Queued: `interaction-blue-50`
- Queue Processing: `orange-40`
- Queue Merged: `success-green-50`
- Queue Failed: `danger-orange-50`

### Typography

- **Red Hat Display** — headings (h1-h3), navigation, prominent UI elements
- **Red Hat Text** — body text, descriptions, table content
- **Red Hat Mono** — code, commit SHAs, technical identifiers, terminal output
- Load from Google Fonts or self-host

### Spacing & Layout

- 8px base grid
- Consistent padding: 16px (sm), 24px (md), 32px (lg)
- Border radius: 4px (subtle)
- Max content width: 1440px
- Sidebar width: 240px (collapsible)

## Deliverables

### 1. Design System Foundation

Create `web/src/lib/design-system.css`:
- CSS custom properties for all Red Hat colors
- Typography scale with Red Hat font families
- Spacing tokens
- Component primitives (buttons, inputs, badges, cards, tables)
- Dark theme as default (light theme future consideration)

### 2. Navigation & Layout Redesign

**Sidebar:**
- Red Hat-styled sidebar with icon + label for each section
- Collapsible to icon-only mode
- Active page indicator (red-50 left border)
- Grouped sections: Overview | Source Control | Agents | Operations | Admin
- Bottom: server status indicator, user avatar (when auth enabled)

**Top bar:**
- Gyre logo/wordmark
- Breadcrumb navigation
- Search (cmd+k)
- Notifications bell
- User menu (when auth enabled)

**Layout:**
- Responsive grid: sidebar + main content
- Content area with proper max-width and padding
- Consistent page headers with title + description + actions

### 3. Dashboard Home (Overview)

Replace the default Activity Feed with a proper dashboard home:
- **Key metrics cards:** Active agents, open tasks, pending MRs, queue depth
- **Recent activity feed** (condensed, last 10 items)
- **Agent health grid:** colored dots showing all agents' status at a glance
- **Merge queue status bar:** visual progress of queued items

### 4. Polished Page Redesigns

**Activity Feed:**
- Timeline-style layout with event type icons
- Color-coded event badges using Red Hat status colors
- Filter bar with pills (clickable to toggle)
- Infinite scroll or paginated

**Agent List:**
- Card grid view (default) + table view toggle
- Agent cards: name, status badge, current task, uptime, last heartbeat sparkline
- Detail panel: slide-out or dedicated page with tabs (Info, Tasks, Activity, Agent Card)

**Task Board:**
- Proper kanban with drag-drop support (or at minimum visual column cards)
- Card design: title, priority chip, assignee avatar/badge, labels as colored pills
- Column headers with count badges
- Swimlane option (group by assignee)

**Repository Detail:**
- Clean tab interface (Branches, Commits, MRs, jj, Settings)
- Commit list: avatar, message (truncated), SHA (mono font), relative time
- Branch list: name, head commit, ahead/behind badges
- MR list: status badge, title, author, branch names

**Merge Request Detail:**
- Two-column: info sidebar + main content
- Diff view: syntax-highlighted (use Prism or Shiki)
- Review comments: threaded, with file/line context
- Action buttons: prominent Approve/Request Changes/Merge
- Status timeline: created → reviewed → approved → queued → merged

**Merge Queue:**
- Priority-ordered list with position numbers
- Visual queue: cards flow left to right (queued → processing → done)
- Processing animation (pulsing indicator)

**Admin Panel:**
- Dashboard cards for system health metrics
- Tabbed interface: Health | Jobs | Audit | SIEM | Compute | Settings
- Job history with expandable rows for logs
- Audit log with advanced filter builder

### 5. Micro-interactions & Polish

- Loading states: skeleton screens (not spinners)
- Empty states: helpful illustrations/messages with CTAs
- Transitions: subtle page/panel transitions (150ms ease)
- Toast notifications for actions (task created, MR merged, etc.)
- Keyboard shortcuts: cmd+k search, shortcuts panel
- Relative timestamps ("3 minutes ago") that update live

### 6. Component Library

Build reusable Svelte components:
- `Button.svelte` — primary (red), secondary (gray), ghost, danger variants
- `Badge.svelte` — status badges with semantic colors
- `Card.svelte` — surface card with optional header/footer
- `Table.svelte` — sortable table with row hover
- `Input.svelte` — text input with label, helper text, error state
- `Select.svelte` — dropdown select
- `Modal.svelte` — dialog with backdrop
- `Toast.svelte` — notification toast system
- `Tabs.svelte` — tabbed interface
- `Skeleton.svelte` — loading skeleton
- `EmptyState.svelte` — empty state with icon + message + CTA
- `Breadcrumb.svelte` — navigation breadcrumbs
- `SearchBar.svelte` — cmd+k search dialog

## Success Criteria

- All pages use Red Hat brand colors and typography consistently
- Navigation is intuitive — user can find any feature in 2 clicks
- Status badges are semantically colored and consistent across all views
- Loading states, empty states, and error states are all handled
- All existing functionality preserved (no regressions)
- Build clean, no console errors

## Non-Goals for M8

- No light theme (dark only for now)
- No drag-and-drop kanban (visual only is fine)
- No accessibility audit (future milestone)
- No mobile-specific responsive design (desktop-first)

## Dependencies

- M7 Production Hardening (complete)
