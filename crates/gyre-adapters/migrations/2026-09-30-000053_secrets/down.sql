-- Reverse secrets table (platform-model.md §7 Secrets Delivery)

DROP INDEX IF EXISTS idx_secrets_scope;
DROP TABLE IF EXISTS secrets;
