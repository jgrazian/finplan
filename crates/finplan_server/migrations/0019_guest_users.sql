-- Guests (spec 17): a user row of its own kind with a normal session, so every
-- route keeps working and deleting a guest cascades like deleting an account.
--
-- `email` and `password_hash` stay NOT NULL: a guest holds placeholders
-- (`guest-<uuid>@guest.invalid` and `!`, which no password verifies) rather
-- than a rebuilt `users` table. Routes that mail or sign in refuse guest rows
-- explicitly; the placeholders are a backstop, not the guard.
ALTER TABLE users ADD COLUMN kind TEXT NOT NULL DEFAULT 'account'
    CHECK (kind IN ('account', 'guest'));
CREATE INDEX users_guest ON users(kind) WHERE kind = 'guest';
