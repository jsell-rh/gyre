-- TASK-171: record the prompt template git SHA on every LLM budget call.
--
-- ui-layout.md §2 "Prompt storage and versioning": "the prompt template
-- version (git SHA) is recorded in cost entries for reproducibility". The
-- BudgetCallRecord audit log is the cost-entry record for LLM calls; without
-- the column, the only provenance carried is the model name, so a diff in
-- generated output caused by a prompt-template change is unattributable.
--
-- NULL when the template came from a DB workspace/tenant override or the
-- hardcoded fallback — those have no git provenance.

ALTER TABLE budget_call_records ADD COLUMN prompt_template_sha TEXT;
