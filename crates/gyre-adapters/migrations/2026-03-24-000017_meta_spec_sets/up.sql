-- Meta-spec sets persisted to DB (M34 Slice 5).
-- Replaces the in-memory Arc<Mutex<HashMap<...>>> in AppState.
CREATE TABLE IF NOT EXISTS meta_spec_sets (
    workspace_id TEXT   NOT NULL PRIMARY KEY,
    json         TEXT   NOT NULL,
    updated_at   BIGINT NOT NULL
);
