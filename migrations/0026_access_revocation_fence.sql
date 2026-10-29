-- Access revocation fence.
--
-- Revoking every device session or reopening local Owner enrollment advances a
-- persistent access epoch. Enrollment authority (an approved pairing) is bound
-- to the epoch current at approval time, so an approval that survives a global
-- revocation cannot create a replacement session. Per-challenge failed-approval
-- counters cap online guessing of the six-digit verification code.
ALTER TABLE device_pairing_challenges ADD COLUMN approved_epoch INTEGER;
ALTER TABLE device_pairing_challenges ADD COLUMN failed_attempts INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS access_security_epoch (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  epoch INTEGER NOT NULL
);
INSERT OR IGNORE INTO access_security_epoch(id, epoch) VALUES (1, 0);
