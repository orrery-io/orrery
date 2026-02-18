ALTER TABLE execution_history ADD COLUMN element_name VARCHAR(255);
ALTER TABLE execution_history ADD COLUMN ordering INT NOT NULL DEFAULT 0;
