-- Bind the exact provider-generated request body to a Core turn reservation.
ALTER TABLE provider_turn_reservations
  ADD COLUMN actual_request_sha256 TEXT NOT NULL DEFAULT '';
