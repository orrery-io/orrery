CREATE TABLE process_definitions (
    id          VARCHAR(255) PRIMARY KEY,
    version     INT NOT NULL DEFAULT 1,
    bpmn_xml    TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE process_instances (
    id                    VARCHAR(255) PRIMARY KEY,
    process_definition_id VARCHAR(255) NOT NULL REFERENCES process_definitions(id),
    state                 VARCHAR(50)  NOT NULL,
    variables             JSONB        NOT NULL DEFAULT '{}',
    active_element_ids    JSONB        NOT NULL DEFAULT '[]',
    created_at            TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    ended_at              TIMESTAMPTZ
);

CREATE INDEX idx_instances_state ON process_instances(state);
CREATE INDEX idx_instances_def   ON process_instances(process_definition_id);
