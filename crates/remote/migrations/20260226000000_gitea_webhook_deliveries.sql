CREATE TABLE IF NOT EXISTS gitea_webhook_deliveries (
    delivery_id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_gitea_webhook_deliveries_created_at
    ON gitea_webhook_deliveries (created_at DESC);
