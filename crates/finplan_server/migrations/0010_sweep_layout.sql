-- How the Analysis screen's graphs are laid out over a scenario's sweep.
--
-- The grid is the answer and this is how someone chose to read it: which
-- graphs, of what kind, against which of the swept variables, sliced where.
-- None of it costs a simulation and all of it is work — a screen built up card
-- by card — so it outlives the session the same way the grid it draws does.
--
-- Opaque to the server. Graph kinds, metrics and camera angles are drawing
-- choices the client owns end to end (see `analysis::results`, which keeps the
-- sweep itself deliberately flat for the same reason), so this stores the
-- client's own JSON and never reads into it.

CREATE TABLE sweep_layout (
    scenario_id INTEGER PRIMARY KEY REFERENCES scenarios(id) ON DELETE CASCADE,
    user_id     TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- A JSON array of the client's graph specs.
    graphs      TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);
