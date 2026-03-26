-- SQLite does not support DROP TABLE with cascade easily; drop in reverse dependency order.
DROP TABLE IF EXISTS graph_deltas;
DROP TABLE IF EXISTS graph_edges;
DROP TABLE IF EXISTS graph_nodes;
