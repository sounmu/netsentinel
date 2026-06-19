-- Refresh-token cleanup prunes by expires_at only. The user/expires index
-- serves per-user session work, but cannot provide a direct expires_at range.
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_expires_at
    ON refresh_tokens(expires_at);
