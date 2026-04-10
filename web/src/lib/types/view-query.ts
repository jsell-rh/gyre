/**
 * TypeScript types for the ViewQuery grammar.
 *
 * These mirror the Rust types defined in crates/gyre-common/src/view_query.rs.
 * The LLM generates view queries; the renderer executes them deterministically.
 *
 * Scope uses discriminated unions on the `type` field, matching the Rust
 * #[serde(tag = "type", rename_all = "snake_case")] serialization.
 */

// ── Scope ────────────────────────────────────────────────────────────────────

/** Show everything. */
export interface ScopeAll {
  type: 'all';
}

/** BFS from a node along specified edges. */
export interface ScopeFocus {
  type: 'focus';
  /** Node name, qualified_name, or computed reference like "$clicked", "$selected". */
  node: string;
  edges?: string[];
  /** "incoming" | "outgoing" | "both". Default: "incoming". */
  direction?: string;
  /** BFS depth limit. Default: 5. Max: 100. */
  depth?: number;
}

/** Show nodes matching node_types, name pattern, or a computed set. */
export interface ScopeFilter {
  type: 'filter';
  node_types?: string[];
  /** Computed expression like "$intersect($where(complexity, '>', 20), $test_unreachable)". */
  computed?: string;
  /** Substring match on name or qualified_name (case-insensitive). */
  name_pattern?: string;
}

/** Nodes NOT reachable from any test function. */
export interface ScopeTestGaps {
  type: 'test_gaps';
}

/** Changes between two commits. */
export interface ScopeDiff {
  type: 'diff';
  from_commit: string;
  to_commit: string;
}

/** Cross-cutting concept from seed nodes expanded along edges. */
export interface ScopeConcept {
  type: 'concept';
  seed_nodes: string[];
  expand_edges?: string[];
  /** Expansion depth. Default: 2. Max: 100. */
  expand_depth?: number;
  /** "incoming" | "outgoing" | "both". Default: "both". */
  expand_direction?: string;
}

/** Discriminated union of all scope types. */
export type Scope =
  | ScopeAll
  | ScopeFocus
  | ScopeFilter
  | ScopeTestGaps
  | ScopeDiff
  | ScopeConcept;

// ── Emphasis ─────────────────────────────────────────────────────────────────

/** How matched/unmatched nodes are styled. */
export interface HighlightStyle {
  color?: string;
  label?: string;
}

export interface Highlight {
  matched?: HighlightStyle;
}

export interface HeatConfig {
  /** Metric name: incoming_calls, complexity, test_fragility, churn, etc. */
  metric: string;
  /** Color palette name: "blue-red", "green-yellow-red", etc. Default: "blue-red". */
  palette?: string;
}

export interface BadgeConfig {
  /** Template string, e.g. "{{count}} calls". */
  template?: string;
  metric?: string;
}

export interface Emphasis {
  highlight?: Highlight;
  /** Opacity for non-matched nodes (0.0-1.0). */
  dim_unmatched?: number;
  /** Array of colors by BFS depth. */
  tiered_colors?: string[];
  /** Color all nodes by metric. */
  heat?: HeatConfig;
  /** Attach text labels. */
  badges?: BadgeConfig;
}

// ── Edges ────────────────────────────────────────────────────────────────────

export interface EdgeFilter {
  /** Edge types to show (inclusion). */
  filter?: string[];
  /** Edge types to exclude (exclusion). Applied after filter. */
  exclude?: string[];
}

// ── Zoom ─────────────────────────────────────────────────────────────────────

/** Zoom can be a named string ("fit", "current") or a numeric level. */
export type Zoom = string | { level: number };

// ── Annotation ───────────────────────────────────────────────────────────────

export interface ViewAnnotation {
  title?: string;
  description?: string;
}

// ── Groups ───────────────────────────────────────────────────────────────────

export interface ViewGroup {
  name: string;
  /** Node names or patterns to include in this group. */
  nodes?: string[];
  color?: string;
  label?: string;
}

// ── Callouts ─────────────────────────────────────────────────────────────────

export interface ViewCallout {
  node: string;
  text: string;
  color?: string;
}

// ── Narrative ────────────────────────────────────────────────────────────────

export interface NarrativeStep {
  node: string;
  text: string;
  order?: number;
}

// ── Top-level ViewQuery ──────────────────────────────────────────────────────

/** A complete view query that the renderer executes. */
export interface ViewQuery {
  scope: Scope;
  emphasis?: Emphasis;
  edges?: EdgeFilter;
  zoom?: Zoom;
  annotation?: ViewAnnotation;
  groups?: ViewGroup[];
  callouts?: ViewCallout[];
  narrative?: NarrativeStep[];
}
