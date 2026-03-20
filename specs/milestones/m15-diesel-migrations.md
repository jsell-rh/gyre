# M15: Diesel ORM + Database Migrations

## Goal

Replace hand-rolled rusqlite with Diesel ORM for compile-time schema validation,
type-safe queries, and dual-backend support (SQLite + PostgreSQL). Add proper
migration framework with up/down pairs, auto-migrate on startup, and multi-tenancy.

## Reference

Full spec: [`specs/development/database-migrations.md`](../development/database-migrations.md)

## Deliverables

### M15.1 Diesel Foundation

Replace rusqlite with Diesel in gyre-adapters.

- Add `diesel` + `diesel_migrations` to Cargo.toml (features: sqlite, postgres, r2d2)
- Generate Diesel migrations matching current 8 inline migrations
- Generate `schema.rs` from resulting schema
- Rewrite all 16 adapter implementations to use Diesel query builder
- Auto-migrate on server startup with lock
- Downgrade protection (refuse to start if DB ahead of binary)
- All existing tests pass against Diesel SQLite

### M15.2 PostgreSQL Support

Add Postgres as an alternative backend.

- Diesel Postgres backend configuration via `GYRE_DATABASE_URL=postgres://...`
- Same adapter code works for both backends (Diesel abstraction)
- CI tests run against both SQLite and Postgres
- Connection pooling via r2d2

### M15.3 Multi-Tenancy

Add tenant_id to all tenant-scoped tables.

- Migration adds `tenant_id TEXT NOT NULL DEFAULT 'default'` to scoped tables
- Tenant resolution from auth context (JWT claim, user lookup, agent lookup)
- Query middleware injects tenant filter
- System tenant for admin cross-tenant access

## Acceptance Criteria

- [ ] All queries use Diesel type-safe builder (no raw SQL in adapters)
- [ ] `diesel migration run` + `diesel migration revert` work for all migrations
- [ ] Server starts with both SQLite and Postgres database URLs
- [ ] Existing data preserved on migration from rusqlite
- [ ] Multi-tenant queries filter by tenant_id
- [ ] 554+ tests pass on both backends
