-- Add event_gateway_group_id to subscription tables for XOR cancellation
-- When an event-based gateway fires, all sibling subscriptions sharing the
-- same group ID are cancelled atomically.

ALTER TABLE message_subscriptions
    ADD COLUMN event_gateway_group_id VARCHAR(255);

ALTER TABLE scheduled_timers
    ADD COLUMN event_gateway_group_id VARCHAR(255);

ALTER TABLE signal_subscriptions
    ADD COLUMN event_gateway_group_id VARCHAR(255);
