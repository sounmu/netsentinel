-- Forced password change after an out-of-band admin reset.
--
-- `reset-admin-password` (a server CLI subcommand) issues a temporary
-- password and sets this flag. While it is 1 the account can authenticate
-- but the API refuses everything except reading its own identity and
-- setting a new password, so a leaked temporary credential cannot be used
-- to operate the instance.
--
-- The reset is deliberately NOT reachable from the web. NetSentinel is a
-- single-admin tool; a self-service reset on the login page would let
-- anyone who can reach that page take the instance over. Running a command
-- on the host proves ownership, which is the property we actually need.

ALTER TABLE users
    ADD COLUMN must_change_password INTEGER NOT NULL DEFAULT 0;
