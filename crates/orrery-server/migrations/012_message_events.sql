-- 012_message_events.sql

-- Revise message_subscriptions: replace variables-blob correlation with a single key value
ALTER TABLE message_subscriptions
    ADD COLUMN message_name        VARCHAR(255),
    ADD COLUMN correlation_key_value VARCHAR(255);

-- Backfill: copy existing message_ref → message_name (best effort)
UPDATE message_subscriptions SET message_name = message_ref WHERE message_name IS NULL;

ALTER TABLE message_subscriptions
    ALTER COLUMN message_name SET NOT NULL;

ALTER TABLE message_subscriptions
    DROP COLUMN message_ref,
    DROP COLUMN correlation_data;

DROP INDEX IF EXISTS idx_msg_sub_ref;
CREATE INDEX idx_msg_sub_name ON message_subscriptions(message_name)
    WHERE consumed_at IS NULL;

-- New: message_start_definitions (parallels timer_start_definitions)
CREATE TABLE message_start_definitions (
    id                   VARCHAR(255) PRIMARY KEY,
    process_def_key      VARCHAR(255) NOT NULL,
    process_def_version  INT          NOT NULL,
    element_id           VARCHAR(255) NOT NULL,
    message_name         VARCHAR(255) NOT NULL,
    enabled              BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE (process_def_key, process_def_version, element_id)
);

CREATE INDEX idx_msg_start_name ON message_start_definitions(message_name)
    WHERE enabled = TRUE;

-- New: message_boundary_subscriptions (one row per active task with a message boundary)
CREATE TABLE message_boundary_subscriptions (
    id                    VARCHAR(255) PRIMARY KEY,
    process_instance_id   VARCHAR(255) NOT NULL REFERENCES process_instances(id),
    element_id            VARCHAR(255) NOT NULL,  -- the MessageBoundaryEvent BPMN id
    attached_to_element   VARCHAR(255) NOT NULL,  -- the task element id
    message_name          VARCHAR(255) NOT NULL,
    correlation_key_value VARCHAR(255),
    is_interrupting       BOOLEAN      NOT NULL DEFAULT TRUE,
    consumed_at           TIMESTAMPTZ,
    created_at            TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_msg_boundary_name ON message_boundary_subscriptions(message_name)
    WHERE consumed_at IS NULL;
