-- Revert TASK-171: drop the prompt template provenance column.

ALTER TABLE budget_call_records DROP COLUMN prompt_template_sha;
