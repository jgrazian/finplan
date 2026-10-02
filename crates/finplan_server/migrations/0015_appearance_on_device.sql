-- Appearance moves off the account and onto the device.
--
-- Light/dark/system is a fact about the screen in front of you, so the web
-- app now keeps it in local storage beside the dark style, and the accent
-- choice is gone: the palette is one statement blue. Neither column is read
-- any more.
ALTER TABLE users DROP COLUMN theme_mode;
ALTER TABLE users DROP COLUMN accent;
