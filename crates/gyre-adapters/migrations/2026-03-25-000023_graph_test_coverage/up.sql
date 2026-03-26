-- M30+: Persist knowledge graph tables with test_coverage column.
-- Creates graph_nodes/graph_edges/graph_deltas tables including
-- test_coverage REAL (0.0–1.0, NULL when unavailable).

CREATE TABLE IF NOT EXISTS graph_nodes (
    id                TEXT PRIMARY KEY NOT NULL,
    repo_id           TEXT NOT NULL,
    node_type         TEXT NOT NULL,
    name              TEXT NOT NULL,
    qualified_name    TEXT NOT NULL,
    file_path         TEXT NOT NULL,
    line_start        INTEGER NOT NULL,
    line_end          INTEGER NOT NULL,
    visibility        TEXT NOT NULL DEFAULT 'public',
    doc_comment       TEXT,
    spec_path         TEXT,
    spec_confidence   TEXT NOT NULL DEFAULT 'none',
    last_modified_sha TEXT NOT NULL,
    last_modified_by  TEXT,
    last_modified_at  INTEGER NOT NULL,
    created_sha       TEXT NOT NULL,
    created_at        INTEGER NOT NULL,
    complexity        INTEGER,
    churn_count_30d   INTEGER NOT NULL DEFAULT 0,
    test_coverage     REAL
);

CREATE TABLE IF NOT EXISTS graph_edges (
    id        TEXT PRIMARY KEY NOT NULL,
    repo_id   TEXT NOT NULL,
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    edge_type TEXT NOT NULL,
    metadata  TEXT
);

CREATE TABLE IF NOT EXISTS graph_deltas (
    id         TEXT PRIMARY KEY NOT NULL,
    repo_id    TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    timestamp  INTEGER NOT NULL,
    agent_id   TEXT,
    spec_ref   TEXT,
    delta_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_graph_nodes_repo ON graph_nodes(repo_id);
CREATE INDEX IF NOT EXISTS idx_graph_nodes_spec ON graph_nodes(spec_path);
CREATE INDEX IF NOT EXISTS idx_graph_edges_repo ON graph_edges(repo_id);
CREATE INDEX IF NOT EXISTS idx_graph_edges_source ON graph_edges(source_id);
CREATE INDEX IF NOT EXISTS idx_graph_edges_target ON graph_edges(target_id);
CREATE INDEX IF NOT EXISTS idx_graph_deltas_repo ON graph_deltas(repo_id);
CREATE INDEX IF NOT EXISTS idx_graph_deltas_ts ON graph_deltas(timestamp);
