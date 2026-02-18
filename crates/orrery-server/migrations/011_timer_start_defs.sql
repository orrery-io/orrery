CREATE TABLE timer_start_definitions (
    id                  TEXT        PRIMARY KEY,
    process_def_key     TEXT        NOT NULL,
    process_def_version INT         NOT NULL,
    element_id          TEXT        NOT NULL,
    timer_kind          TEXT        NOT NULL,
    expression          TEXT        NOT NULL,
    next_due_at         TIMESTAMPTZ,
    enabled             BOOL        NOT NULL DEFAULT TRUE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (process_def_key, process_def_version, element_id)
);

CREATE INDEX idx_timer_start_due ON timer_start_definitions (next_due_at)
    WHERE enabled = TRUE AND next_due_at IS NOT NULL;
