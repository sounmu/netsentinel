-- Security hardening (key separation, response signing, enrollment binding).
--
-- server_secrets: hub-only key material generated on first boot. The user
-- session signing key lives here so it is never the value that legacy agents
-- hold as JWT_SECRET; a monitored host that leaks its secret can no longer
-- sign dashboard tokens.
CREATE TABLE IF NOT EXISTS server_secrets (
    name        TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
) STRICT;

-- Set once the hub has verified a signed response from this agent. From then
-- on an unsigned response is rejected, so an on-path attacker cannot strip
-- the signature to fall back to the unauthenticated legacy behaviour.
ALTER TABLE hosts ADD COLUMN agent_signs_responses INTEGER NOT NULL DEFAULT 0;

-- A claim may only replace an existing host's secret when the admin who
-- minted the token opted into re-enrollment.
ALTER TABLE agent_enrollment_tokens ADD COLUMN allow_existing_host INTEGER NOT NULL DEFAULT 0;
