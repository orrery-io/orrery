-- Event subprocess subscriptions.
-- Rows exist while the parent scope is active; deleted when scope completes or is cancelled.
-- trigger_type: 'message' | 'signal' | 'timer'
CREATE TABLE event_subprocess_subscriptions (
    id                   TEXT PRIMARY KEY,
    process_instance_id  TEXT NOT NULL REFERENCES process_instances(id),
    esp_id               TEXT NOT NULL,
    scope_id             TEXT,
    trigger_type         TEXT NOT NULL,
    message_name         TEXT,
    correlation_key      TEXT,
    signal_ref           TEXT,
    timer_expression     TEXT,
    timer_kind           TEXT,
    due_at               TIMESTAMPTZ,
    is_interrupting      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (process_instance_id, esp_id)
);

CREATE INDEX idx_esp_subs_instance ON event_subprocess_subscriptions (process_instance_id);
CREATE INDEX idx_esp_subs_signal ON event_subprocess_subscriptions (signal_ref)
    WHERE trigger_type = 'signal';
CREATE INDEX idx_esp_subs_message ON event_subprocess_subscriptions (message_name)
    WHERE trigger_type = 'message';
CREATE INDEX idx_esp_subs_timer ON event_subprocess_subscriptions (due_at)
    WHERE trigger_type = 'timer';
