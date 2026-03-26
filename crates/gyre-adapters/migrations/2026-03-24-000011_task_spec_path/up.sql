-- Add spec_path to tasks so spec lifecycle tasks are linked to their spec.
ALTER TABLE tasks ADD COLUMN spec_path TEXT;
