-- 008_definition_versioning.sql
-- Wipe and recreate with composite-PK versioned definitions.
-- All incremental changes from 002-007 are consolidated here.

DROP TABLE IF EXISTS signal_subscriptions CASCADE;
DROP TABLE IF EXISTS message_subscriptions CASCADE;
DROP TABLE IF EXISTS scheduled_timers CASCADE;
DROP TABLE IF EXISTS tasks CASCADE;
DROP TABLE IF EXISTS execution_history CASCADE;
DROP TABLE IF EXISTS process_instances CASCADE;
DROP TABLE IF EXISTS process_definitions CASCADE;

-- One immutable row per (id, version)
CREATE TABLE process_definitions (
    id          VARCHAR(255) NOT NULL,
    version     INT          NOT NULL,
    bpmn_xml    TEXT         NOT NULL,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id, version)
);

-- Instances pinned to exact (definition_id, version)
CREATE TABLE process_instances (
    id                         VARCHAR(255) PRIMARY KEY,
    process_definition_id      VARCHAR(255) NOT NULL,
    process_definition_version INT          NOT NULL,
    state                      VARCHAR(50)  NOT NULL,
    variables                  JSONB        NOT NULL DEFAULT '{}',
    active_element_ids         JSONB        NOT NULL DEFAULT '[]',
    created_at                 TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    ended_at                   TIMESTAMPTZ,
    error_message              TEXT,
    FOREIGN KEY (process_definition_id, process_definition_version)
        REFERENCES process_definitions (id, version)
);

CREATE INDEX idx_instances_state ON process_instances(state);
CREATE INDEX idx_instances_def   ON process_instances(process_definition_id, process_definition_version);

CREATE TABLE execution_history (
    id                   BIGSERIAL    PRIMARY KEY,
    process_instance_id  VARCHAR(255) NOT NULL REFERENCES process_instances(id),
    element_id           VARCHAR(255) NOT NULL,
    element_type         VARCHAR(100) NOT NULL,
    event_type           VARCHAR(50)  NOT NULL,
    variables_snapshot   JSONB        NOT NULL DEFAULT '{}',
    occurred_at          TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_history_instance ON execution_history(process_instance_id, occurred_at);

CREATE TABLE tasks (
    id                   VARCHAR(255) PRIMARY KEY,
    process_instance_id  VARCHAR(255) NOT NULL REFERENCES process_instances(id),
    element_id           VARCHAR(255) NOT NULL,
    element_type         VARCHAR(50)  NOT NULL DEFAULT 'SERVICE_TASK',
    state                VARCHAR(50)  NOT NULL DEFAULT 'CREATED',
    claimed_by           VARCHAR(255),
    variables            JSONB        NOT NULL DEFAULT '{}',
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    claimed_at           TIMESTAMPTZ,
    completed_at         TIMESTAMPTZ,
    retry_count          INT          NOT NULL DEFAULT 0,
    max_retries          INT          NOT NULL DEFAULT 0,
    next_retry_at        TIMESTAMPTZ,
    topic                VARCHAR(255),
    locked_until         TIMESTAMPTZ
);

CREATE INDEX idx_tasks_state    ON tasks(state);
CREATE INDEX idx_tasks_instance ON tasks(process_instance_id);

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

CREATE TABLE message_subscriptions (
    id                   VARCHAR(255) PRIMARY KEY,
    process_instance_id  VARCHAR(255) NOT NULL REFERENCES process_instances(id),
    element_id           VARCHAR(255) NOT NULL,
    message_ref          VARCHAR(255) NOT NULL,
    correlation_data     JSONB        NOT NULL DEFAULT '{}',
    consumed_at          TIMESTAMPTZ,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_msg_sub_ref ON message_subscriptions(message_ref)
    WHERE consumed_at IS NULL;

CREATE TABLE signal_subscriptions (
    id                   VARCHAR(255) PRIMARY KEY,
    process_instance_id  VARCHAR(255) NOT NULL REFERENCES process_instances(id),
    element_id           VARCHAR(255) NOT NULL,
    signal_ref           VARCHAR(255) NOT NULL,
    consumed_at          TIMESTAMPTZ,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sig_sub_ref ON signal_subscriptions(signal_ref)
    WHERE consumed_at IS NULL;
