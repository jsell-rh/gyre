# Business Continuity

Gyre needs **BCP primitives** - reusable, composable building blocks, not ad-hoc scripts:

- **Snapshot/restore** - atomic, point-in-time capture of server state (DB, secrets, config)
- **Health checks / liveness probes** - standardized across server, agents, background jobs
- **Graceful degradation** - what do agents do when the central server is unreachable?
- **Automated failover** (if needed, or justify why vertical + simplicity is sufficient)
- **Data retention policies** - configurable per data type (audit logs, context windows, job history)
- **Export/migration** - portable state, no vendor lock-in to a specific backing store

## Open BCP Questions

- [ ] What's the target RTO/RPO?
- [ ] Should agents be able to operate autonomously (disconnected mode) if the server is down?
- [ ] How do snapshots interact with SIEM forwarding - is the SIEM the backup for audit data?
