-- Tracks process instances waiting for a message event.
-- correlation_data is a snapshot of instance variables at subscription time;
-- used to match POST /v1/messages correlation keys via JSONB containment (@>).
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

-- Tracks process instances waiting for a signal event (broadcast).
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
