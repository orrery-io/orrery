-- Scheduled timers for timer intermediate events
CREATE TABLE scheduled_timers (
    id                   VARCHAR(255) PRIMARY KEY,
    process_instance_id  VARCHAR(255) NOT NULL REFERENCES process_instances(id),
    element_id           VARCHAR(255) NOT NULL,
    due_at               TIMESTAMPTZ  NOT NULL,
    fired                BOOLEAN      NOT NULL DEFAULT FALSE,
    fired_at             TIMESTAMPTZ,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_timers_due ON scheduled_timers(due_at) WHERE fired = FALSE;

-- Retry support on tasks table
ALTER TABLE tasks
    ADD COLUMN retry_count   INT NOT NULL DEFAULT 0,
    ADD COLUMN max_retries   INT NOT NULL DEFAULT 0,
    ADD COLUMN next_retry_at TIMESTAMPTZ;
