-- TASK-273 (admin-panel.md §Background Jobs: remote agent cleanup):
-- durable spawned-instance handles. At spawn the server records
-- (agent_id, backend descriptor, handle); the remote_agent_cleanup job
-- kills still-recorded-running instances whose agent is Dead/Stopped or
-- absent — including orphans left by a server restart, which the
-- in-memory process registry cannot see.

CREATE TABLE agent_instances (
    agent_id TEXT PRIMARY KEY NOT NULL,
    backend TEXT NOT NULL,
    target_type TEXT NOT NULL,
    handle_id TEXT NOT NULL,
    pid INTEGER,
    spawned_at BIGINT NOT NULL,
    terminated_at BIGINT,
    terminated_reason TEXT
);

CREATE INDEX idx_agent_instances_terminated ON agent_instances (terminated_at);
