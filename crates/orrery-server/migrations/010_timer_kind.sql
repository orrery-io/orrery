-- Rename duration column to expression (more accurate for all timer types)
ALTER TABLE scheduled_timers RENAME COLUMN duration TO expression;

-- Add timer_kind discriminant; default 'duration' for all existing rows
ALTER TABLE scheduled_timers
    ADD COLUMN timer_kind TEXT NOT NULL DEFAULT 'duration';
