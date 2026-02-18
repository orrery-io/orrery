-- Add business_key to process_instances for external correlation
ALTER TABLE process_instances
    ADD COLUMN business_key VARCHAR(255);

CREATE INDEX idx_instances_business_key ON process_instances(business_key)
    WHERE business_key IS NOT NULL;
