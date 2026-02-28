CREATE TABLE IF NOT EXISTS gitea_review_feedback (
    review_id UUID NOT NULL,
    terminal_state TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    posted_at TIMESTAMPTZ,
    last_error TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (review_id, terminal_state)
);
