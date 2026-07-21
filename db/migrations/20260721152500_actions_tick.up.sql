-- The watchdog's oscillation check (a ride's set_ride_price direction
-- reversing too many times within a tick window) needs the game tick an
-- action was authorized at -- nothing on the ledger recorded that before
-- now (expiry_tick is a deadline, not the authorization moment).
ALTER TABLE actions ADD COLUMN tick BIGINT NOT NULL;
