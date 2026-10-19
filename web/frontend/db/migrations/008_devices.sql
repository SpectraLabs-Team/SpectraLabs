-- M19 task 11: recognise devices, to email the owner when someone signs in from a new one.
-- A device is a browser holding the long-lived random `spectra_device` cookie; only its keyed hash
-- is stored. Clearing cookies or a private window counts as a new device (as with most services).

CREATE TABLE known_devices (
  user_id       uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  device_hash   bytea       NOT NULL,
  first_seen_at timestamptz NOT NULL DEFAULT now(),
  last_seen_at  timestamptz NOT NULL DEFAULT now(),
  user_agent    text,
  ip            text,
  PRIMARY KEY (user_id, device_hash)
);

-- Which device a session belongs to (for "this device" and "first seen" in the devices list).
ALTER TABLE sessions ADD COLUMN device_hash bytea;
