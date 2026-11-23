CREATE TABLE sessions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    session_uid  TEXT    NOT NULL,
    game_year    INTEGER NOT NULL,
    track_id     INTEGER NOT NULL,
    track_name   TEXT    NOT NULL,
    session_type TEXT    NOT NULL,
    total_laps   INTEGER NOT NULL,
    started_at   TEXT    NOT NULL
);
CREATE INDEX idx_sessions_uid ON sessions (session_uid);

CREATE TABLE laps (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id     INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    car_index      INTEGER NOT NULL,
    driver         TEXT    NOT NULL,
    lap            INTEGER NOT NULL,
    lap_ms         INTEGER NOT NULL,
    s1_ms          INTEGER,
    s2_ms          INTEGER,
    s3_ms          INTEGER,
    compound       TEXT,
    tyre_age       INTEGER,
    wear_fl        REAL,
    wear_fr        REAL,
    wear_rl        REAL,
    wear_rr        REAL,
    fuel_kg        REAL,
    ers_harvest_j  REAL,
    ers_deploy_j   REAL,
    valid          INTEGER NOT NULL,
    position       INTEGER NOT NULL,
    session_time   REAL    NOT NULL,
    UNIQUE (session_id, car_index, lap)
);

CREATE TABLE stints (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    car_index   INTEGER NOT NULL,
    compound    TEXT    NOT NULL,
    start_lap   INTEGER NOT NULL,
    end_lap     INTEGER
);

CREATE TABLE radio_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  INTEGER REFERENCES sessions (id) ON DELETE SET NULL,
    at_ms       INTEGER NOT NULL,
    priority    TEXT    NOT NULL,
    source      TEXT    NOT NULL,
    text        TEXT    NOT NULL,
    context     TEXT,
    outcome     TEXT    NOT NULL
);

CREATE TABLE track_outlines (
    track_id    INTEGER PRIMARY KEY,
    length_m    REAL    NOT NULL,
    points      TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
);
