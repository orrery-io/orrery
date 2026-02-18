-- Signal start definitions (mirrors timer_start_definitions and message_start_definitions)
CREATE TABLE signal_start_definitions (
    id                   VARCHAR(255) PRIMARY KEY,
    process_def_key      VARCHAR(255) NOT NULL,
    process_def_version  INT          NOT NULL,
    element_id           VARCHAR(255) NOT NULL,
    signal_ref           VARCHAR(255) NOT NULL,
    enabled              BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE (process_def_key, process_def_version, element_id)
);

CREATE INDEX idx_sig_start_ref ON signal_start_definitions(signal_ref)
    WHERE enabled = TRUE;

-- Signal boundary subscriptions (mirrors message_boundary_subscriptions)
CREATE TABLE signal_boundary_subscriptions (
    id                    VARCHAR(255) PRIMARY KEY,
    process_instance_id   VARCHAR(255) NOT NULL REFERENCES process_instances(id),
    element_id            VARCHAR(255) NOT NULL,
    attached_to_element   VARCHAR(255) NOT NULL,
    signal_ref            VARCHAR(255) NOT NULL,
    is_interrupting       BOOLEAN      NOT NULL DEFAULT TRUE,
    consumed_at           TIMESTAMPTZ,
    created_at            TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sig_boundary_ref ON signal_boundary_subscriptions(signal_ref)
    WHERE consumed_at IS NULL;
