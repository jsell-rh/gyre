-- agent-runtime.md amendments row (platform-model.md §2 Persona): the
-- /api/v1/personas endpoints are removed — replaced entirely by the
-- meta-spec registry's meta:persona kind. The registry persona kind must
-- carry the Persona operational fields (model, temperature, max_tokens,
-- budget) so they remain editable after the personas routes are removed.
-- The columns are nullable and ignored by every non-persona kind; `budget`
-- holds the JSON serialization of BudgetConfig.
ALTER TABLE meta_specs ADD COLUMN model TEXT;
ALTER TABLE meta_specs ADD COLUMN temperature DOUBLE PRECISION;
ALTER TABLE meta_specs ADD COLUMN max_tokens INTEGER;
ALTER TABLE meta_specs ADD COLUMN budget TEXT;
