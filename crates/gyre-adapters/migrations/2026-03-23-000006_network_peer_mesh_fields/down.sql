-- Revert mesh_ip and is_stale columns (SQLite does not support DROP COLUMN in older versions,
-- so we recreate the table)
CREATE TABLE network_peers_old AS SELECT
    id, agent_id, wireguard_pubkey, endpoint, allowed_ips, registered_at, last_seen
FROM network_peers;
DROP TABLE network_peers;
ALTER TABLE network_peers_old RENAME TO network_peers;
