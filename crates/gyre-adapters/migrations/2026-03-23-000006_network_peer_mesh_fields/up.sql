-- M26: Add mesh_ip and is_stale to network_peers
ALTER TABLE network_peers ADD COLUMN mesh_ip TEXT;
ALTER TABLE network_peers ADD COLUMN is_stale BOOLEAN NOT NULL DEFAULT FALSE;
