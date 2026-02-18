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
    completed_at         TIMESTAMPTZ
);

CREATE INDEX idx_tasks_state ON tasks(state);
CREATE INDEX idx_tasks_instance ON tasks(process_instance_id);
