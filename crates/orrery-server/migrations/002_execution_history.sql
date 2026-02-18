CREATE TABLE execution_history (
    id                   BIGSERIAL    PRIMARY KEY,
    process_instance_id  VARCHAR(255) NOT NULL REFERENCES process_instances(id),
    element_id           VARCHAR(255) NOT NULL,
    element_type         VARCHAR(100) NOT NULL,
    event_type           VARCHAR(50)  NOT NULL, -- ELEMENT_ACTIVATED, ELEMENT_COMPLETED
    variables_snapshot   JSONB        NOT NULL DEFAULT '{}',
    occurred_at          TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_history_instance ON execution_history(process_instance_id, occurred_at);
