-- Revert TASK-273: drop the OAuth state table.

DROP TABLE IF EXISTS oauth_states;
