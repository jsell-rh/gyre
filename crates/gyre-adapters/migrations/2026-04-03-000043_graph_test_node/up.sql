-- Add test_node column to graph_nodes for test coverage analysis
ALTER TABLE graph_nodes ADD COLUMN test_node BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX IF NOT EXISTS idx_graph_nodes_test ON graph_nodes(repo_id, test_node) WHERE test_node = TRUE;
