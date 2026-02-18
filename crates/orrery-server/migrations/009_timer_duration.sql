-- Store the original ISO 8601 duration expression alongside each scheduled timer
ALTER TABLE scheduled_timers ADD COLUMN duration VARCHAR(255);
