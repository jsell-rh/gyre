-- M38: Time-travel columns for graph_nodes and graph_edges.
--
-- Enables point-in-time graph reconstruction:
--   WHERE first_seen_at <= :ts AND (deleted_at IS NULL OR deleted_at > :ts)
--
-- Nodes and edges are soft-deleted (deleted_at set) rather than removed,
-- so the full change history is preserved forever.

ALTER TABLE graph_nodes ADD COLUMN first_seen_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE graph_nodes ADD COLUMN last_seen_at  INTEGER NOT NULL DEFAULT 0;
ALTER TABLE graph_nodes ADD COLUMN deleted_at    INTEGER;

ALTER TABLE graph_edges ADD COLUMN first_seen_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE graph_edges ADD COLUMN last_seen_at  INTEGER NOT NULL DEFAULT 0;
ALTER TABLE graph_edges ADD COLUMN deleted_at    INTEGER;

-- Index on deleted_at to make active-node queries fast.
CREATE INDEX IF NOT EXISTS idx_graph_nodes_deleted ON graph_nodes(deleted_at);
CREATE INDEX IF NOT EXISTS idx_graph_edges_deleted ON graph_edges(deleted_at);
