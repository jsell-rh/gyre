//! PG-only post-migration schema repairs (task-110 review).
//!
//! Migration `2026-11-13-000063_invitations` originally declared its six
//! timestamp columns (`expires_at`/`created_at`/`accepted_at` on
//! `tenant_invitations` and `workspace_invitations`) as INTEGER. The
//! adapters' row structs and `schema.rs` bind those columns as diesel
//! `BigInt`/i64, and on PostgreSQL `FromSql<BigInt, Pg>` rejects int4's
//! 4-byte binary format — every invitation read failed at runtime while
//! SQLite stayed green (dynamic typing).
//!
//! The shared migration now declares BIGINT for fresh databases, but
//! Diesel records versions, not file contents, so PostgreSQL databases
//! that already ran the INTEGER variant keep their int4 columns until
//! repaired here. This is deliberately a PostgreSQL-only init-path hook
//! (mirroring `search::ensure_search_table`), NOT a shared `migrations/`
//! entry: `ALTER COLUMN ... TYPE` is not valid SQLite DDL, and SQLite
//! needs no repair — INTEGER affinity already stores and reads i64.
//!
//! Idempotent: a no-op when the columns are already BIGINT (fresh
//! databases, or a second run), and a no-op when the tables are absent.

// `QueryableByName`'s derive (diesel_derives 2.3.7) expands to
// `Self { field: field, … }` with spans mapped onto the `ColumnRow`
// declaration below, so clippy::redundant_field_names fires on
// derive-generated code. The generated construction sits in a dummy
// module that inherits neither struct- nor field-level `#[allow]`
// (verified on the postgres/search.rs and postgres/analytics.rs
// siblings), so the lint is scoped off for this whole module instead.
// Nothing in this file writes `field: field` initializations by hand.
#![allow(clippy::redundant_field_names)]

use anyhow::Result;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::Text;
use diesel::OptionalExtension;

/// The invitation timestamp columns that must be BIGINT (int8).
const TIMESTAMP_COLUMNS: &[(&str, &str)] = &[
    ("tenant_invitations", "expires_at"),
    ("tenant_invitations", "created_at"),
    ("tenant_invitations", "accepted_at"),
    ("workspace_invitations", "expires_at"),
    ("workspace_invitations", "created_at"),
    ("workspace_invitations", "accepted_at"),
];

#[derive(QueryableByName)]
struct ColumnRow {
    #[diesel(sql_type = Text)]
    data_type: String,
}

/// Repair the six invitation timestamp columns to BIGINT on PostgreSQL
/// databases that ran the pre-repair INTEGER variant of migration
/// 000063. Runs on every `PgStorage` construction; cheap (six
/// `information_schema` lookups) and a no-op once repaired.
pub(crate) fn ensure_invitation_timestamps_bigint(conn: &mut PgConnection) -> Result<()> {
    for &(table, column) in TIMESTAMP_COLUMNS {
        let row = diesel::sql_query(
            "SELECT data_type FROM information_schema.columns \
             WHERE table_schema = current_schema() \
             AND table_name = $1 AND column_name = $2",
        )
        .bind::<Text, _>(table)
        .bind::<Text, _>(column)
        .get_result::<ColumnRow>(conn)
        .optional()?;
        let Some(row) = row else {
            // Table (or column) absent — migrations have not created it
            // in this schema; nothing to repair.
            continue;
        };
        if row.data_type == "integer" {
            // Table and column names are compile-time constants from
            // TIMESTAMP_COLUMNS — the format! cannot inject input.
            diesel::sql_query(format!(
                "ALTER TABLE {table} ALTER COLUMN {column} TYPE BIGINT"
            ))
            .execute(conn)?;
            tracing::info!(
                table,
                column,
                "repaired invitation timestamp column INTEGER -> BIGINT"
            );
        }
    }
    Ok(())
}
