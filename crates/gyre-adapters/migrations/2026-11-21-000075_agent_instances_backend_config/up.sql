-- TASK-273 review: kill-reconstruction details for spawned agent
-- instances. The remote_agent_cleanup sweep reconstructs a compute target
-- for restart-orphans from the durable record alone; container and
-- Kubernetes kills address global resources (docker rm / kubectl delete
-- pod) whose location depends on connection facts known only at spawn
-- (container runtime; pod namespace / kubectl context / binary path).
-- Without them the reconstructed kill addresses the wrong place, fails,
-- and — because kill_process treated any non-zero exit as success — the
-- orphan was marked terminated while still running. Stored as a JSON
-- TEXT column: NULL = nothing to reconstruct (local backend) or not
-- reconstructable (ssh). Portable across SQLite and PostgreSQL.

ALTER TABLE agent_instances ADD COLUMN backend_config TEXT;
