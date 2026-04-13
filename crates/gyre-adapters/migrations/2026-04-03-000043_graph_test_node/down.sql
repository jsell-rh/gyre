-- SQLite does not support DROP COLUMN in older versions,
-- but diesel migration revert will recreate the table.
-- For newer SQLite:
ALTER TABLE graph_nodes DROP COLUMN test_node;
