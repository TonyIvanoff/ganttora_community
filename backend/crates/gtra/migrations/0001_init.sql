-- =============================================================================
-- 0001_init.sql: the first version of the .gtra file format (format version 1)
-- =============================================================================
--
-- Reference page: docs/reference/en/backend/crates/gtra/migrations/0001_init.md
--
-- WHAT THIS FILE IS
-- A .gtra file is a SQLite database. This file contains the SQL commands that
-- create its tables in a brand-new, empty database. SQL ("Structured Query
-- Language") is the language used to talk to databases. Lines starting with
-- "--" are comments: SQLite ignores them.
--
-- WHERE IT FITS
-- The Rust code in backend/crates/gtra/src/schema.rs embeds this file into the
-- app (with `include_str!`) and runs it when a new .gtra file is created. It
-- runs inside one transaction, so either every table is created or none is.
--
-- RULES FOR THIS FILE (plan.md §5, CLAUDE.md §6)
-- * This file is NEVER edited once it has been released. Every later change
--   to the format is a NEW numbered file: 0002_....sql, 0003_....sql.
--   Files made by older versions of Ganttora are upgraded by running the
--   missing migrations one after another.
-- * This file does NOT set the format version. The migration runner does it
--   (PRAGMA user_version and the 'format_version' row in `meta`), so no
--   migration can forget it.
-- * Every item (task, dependency, calendar, ...) has a UUID as its primary
--   key, stored as text such as '0b7d5c3e-....'. Never a row number.
--
-- VALUE FORMATS USED IN EVERY TABLE
--   UUID        TEXT, lower-case, 36 characters with dashes.
--   true/false  INTEGER 0 or 1 (SQLite has no separate yes/no type).
--   timestamp   TEXT, a moment in UTC, ISO 8601: '2027-02-15T10:42:07Z'.
--               Used for "when did this happen" (created, journal...).
--   date-time   TEXT, project-local wall-clock time without time zone:
--               '2027-03-01T08:00:00'. Used for schedule dates (constraint
--               dates, deadlines), exactly like MS Project, so that a plan
--               does not shift when the file is opened in another time zone.
--   date        TEXT, '2027-03-01'.
--   duration    INTEGER, number of WORKING minutes (2 days on an 8-hour
--               calendar = 960). A separate *_unit column remembers how the
--               user typed it ('d' = days...), only for display.
--   percent     REAL, as a fraction: 1.0 = 100 %, 0.5 = 50 %.
--
-- STRICT TABLES
-- Every table is declared STRICT. Normally SQLite lets you store text in a
-- number column; a STRICT table refuses that, so a bug cannot quietly store
-- the wrong kind of value.
--
-- OVERVIEW OF THE TABLES (PK = primary key, FK = foreign key)
--
--   meta (key PK, value)                 "the cover of the file"
--   project_settings (key PK, value)     project-level options
--
--   calendars (id PK, base_calendar_id FK -> calendars)
--     |-- calendar_week_days (calendar_id FK, weekday)    which weekdays work
--     |     '-- calendar_working_times (calendar_id, weekday) FK -> week_days
--     '-- calendar_exceptions (id PK, calendar_id FK)     holidays etc.
--           '-- calendar_exception_times (exception_id FK)
--
--   project (id PK, calendar_id FK -> calendars)          exactly one row
--
--   tasks (id PK, parent_id FK -> tasks, calendar_id FK -> calendars)
--     '-- dependencies (id PK, predecessor_id FK -> tasks, successor_id FK -> tasks)
--
--   resources (id PK, calendar_id FK -> calendars)
--   assignments (id PK, task_id FK -> tasks, resource_id FK -> resources)
--
--   journal (id PK, ... entity_id)       field-level change records
--   versions (id PK, ...)                snapshots for "restore an earlier version"
--
-- See docs/file-format.md and book section 4.4 "The .gtra file format in depth".
-- =============================================================================


-- -----------------------------------------------------------------------------
-- meta: data ABOUT the file (not about the project's tasks).
-- -----------------------------------------------------------------------------
-- Stored as key/value pairs, like a small dictionary, so new entries can be
-- added later without changing the table.
--
-- Keys written by Ganttora (all values are text):
--   format_version   copy of the schema version, e.g. '1' (the authoritative
--                    number is PRAGMA user_version, see schema.rs)
--   app_version      version of Ganttora that last saved the file, '0.1.0'
--   engine_version   version of the scheduling engine that last saved it
--   project_id       UUID of the project (same as project.id)
--   created          timestamp, when the file was created
--   modified         timestamp, when the file was last saved
--   author           display name from Settings -> User ('' if not set)
CREATE TABLE meta (
    key   TEXT PRIMARY KEY NOT NULL,   -- name of the entry, e.g. 'author'
    value TEXT NOT NULL                -- its value, always stored as text
) STRICT;


-- -----------------------------------------------------------------------------
-- project_settings: options that belong to this project (not to the app).
-- -----------------------------------------------------------------------------
-- Example: the currency symbol, default task type, hours per day.
-- Key/value pairs for the same reason as `meta`. Values are JSON text, so a
-- value can be a number, a word or a small list.
CREATE TABLE project_settings (
    key   TEXT PRIMARY KEY NOT NULL,   -- e.g. 'currency'
    value TEXT NOT NULL                -- JSON, e.g. '"EUR"' or '8'
) STRICT;


-- -----------------------------------------------------------------------------
-- calendars: which hours are working time (book section 2.3).
-- -----------------------------------------------------------------------------
-- A calendar can be based on another one ("base calendar"): a resource
-- calendar for Anna can say "like Standard, but I don't work on Fridays".
-- Anything the calendar does not define itself is taken from its base.
CREATE TABLE calendars (
    id               TEXT PRIMARY KEY NOT NULL,  -- UUID
    name             TEXT NOT NULL,              -- e.g. 'Standard', '24 Hours'
    -- The calendar this one inherits from, or NULL for a complete calendar.
    -- ON DELETE RESTRICT: a calendar that others are based on cannot be
    -- deleted until they are changed (the app asks the user first).
    base_calendar_id TEXT REFERENCES calendars(id) ON DELETE RESTRICT,
    CHECK (base_calendar_id IS NULL OR base_calendar_id <> id)
) STRICT;


-- -----------------------------------------------------------------------------
-- calendar_week_days: for each weekday, is it working or not?
-- -----------------------------------------------------------------------------
-- One row per weekday that the calendar defines itself. A weekday WITHOUT a
-- row is inherited from the base calendar (or non-working if there is none).
--
--   calendar 'Standard':  weekday 1 (Mon) working ... 5 (Fri) working,
--                         6 (Sat) non-working, 7 (Sun) non-working
CREATE TABLE calendar_week_days (
    calendar_id TEXT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    -- ISO weekday number: 1 = Monday ... 7 = Sunday.
    weekday     INTEGER NOT NULL CHECK (weekday BETWEEN 1 AND 7),
    -- 1 = working day (its hours are in calendar_working_times), 0 = day off.
    is_working  INTEGER NOT NULL CHECK (is_working IN (0, 1)),
    -- A "composite" primary key: the PAIR (calendar, weekday) is unique.
    PRIMARY KEY (calendar_id, weekday)
) STRICT;


-- -----------------------------------------------------------------------------
-- calendar_working_times: the working hours of a working weekday.
-- -----------------------------------------------------------------------------
-- Several rows per day, one per block of work, e.g. 08:00-12:00 and
-- 13:00-17:00 (a lunch break in between). Times are minutes after midnight:
-- 08:00 = 480, 12:00 = 720, 24:00 = 1440.
CREATE TABLE calendar_working_times (
    calendar_id  TEXT NOT NULL,
    weekday      INTEGER NOT NULL,
    start_minute INTEGER NOT NULL CHECK (start_minute BETWEEN 0 AND 1439),
    end_minute   INTEGER NOT NULL CHECK (end_minute BETWEEN 1 AND 1440),
    CHECK (start_minute < end_minute),
    PRIMARY KEY (calendar_id, weekday, start_minute),
    -- A two-column foreign key: (calendar_id, weekday) must exist in
    -- calendar_week_days. Deleting that weekday row deletes its times too.
    FOREIGN KEY (calendar_id, weekday)
        REFERENCES calendar_week_days(calendar_id, weekday) ON DELETE CASCADE
) STRICT;


-- -----------------------------------------------------------------------------
-- calendar_exceptions: days that differ from the normal week.
-- -----------------------------------------------------------------------------
-- Holidays (non-working) or special working days, e.g. "Saturday 6 March is
-- a working day". An exception covers a range of dates (one day: start_date =
-- end_date). Exceptions win over the weekly pattern. Recurring exceptions
-- ("every 25 December") are added in a later migration (M1).
CREATE TABLE calendar_exceptions (
    id          TEXT PRIMARY KEY NOT NULL,   -- UUID
    calendar_id TEXT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    name        TEXT NOT NULL DEFAULT '',    -- e.g. 'Easter Monday'
    start_date  TEXT NOT NULL,               -- date, first day (inclusive)
    end_date    TEXT NOT NULL,               -- date, last day (inclusive)
    -- 0 = the whole range is non-working; 1 = working, with the hours in
    -- calendar_exception_times.
    is_working  INTEGER NOT NULL CHECK (is_working IN (0, 1)),
    -- ISO dates compare correctly as text: '2027-03-01' < '2027-03-02'.
    CHECK (start_date <= end_date)
) STRICT;

-- An "index" is like the index at the back of a book: it lets SQLite find
-- all exceptions of one calendar without reading the whole table.
CREATE INDEX idx_calendar_exceptions_calendar ON calendar_exceptions(calendar_id);


-- -----------------------------------------------------------------------------
-- calendar_exception_times: working hours on a working exception day.
-- -----------------------------------------------------------------------------
CREATE TABLE calendar_exception_times (
    exception_id TEXT NOT NULL REFERENCES calendar_exceptions(id) ON DELETE CASCADE,
    start_minute INTEGER NOT NULL CHECK (start_minute BETWEEN 0 AND 1439),
    end_minute   INTEGER NOT NULL CHECK (end_minute BETWEEN 1 AND 1440),
    CHECK (start_minute < end_minute),
    PRIMARY KEY (exception_id, start_minute)
) STRICT;


-- -----------------------------------------------------------------------------
-- project: the project itself. Exactly one row per file.
-- -----------------------------------------------------------------------------
CREATE TABLE project (
    id            TEXT PRIMARY KEY NOT NULL,  -- UUID, same as meta 'project_id'
    name          TEXT NOT NULL DEFAULT '',   -- e.g. 'Garden shed'
    -- 'start': the user gives the start date, the engine calculates the
    -- finish (the usual way). 'finish': the other way round (plan.md §4.2).
    schedule_from TEXT NOT NULL DEFAULT 'start'
                  CHECK (schedule_from IN ('start', 'finish')),
    start_date    TEXT,                       -- date-time; used when schedule_from = 'start'
    finish_date   TEXT,                       -- date-time; used when schedule_from = 'finish'
    status_date   TEXT,                       -- date-time for progress tracking, or NULL
    -- The project calendar: the default working time for all tasks.
    calendar_id   TEXT REFERENCES calendars(id) ON DELETE RESTRICT,
    notes         TEXT NOT NULL DEFAULT ''
) STRICT;


-- -----------------------------------------------------------------------------
-- tasks: one row per task, milestone or summary task.
-- -----------------------------------------------------------------------------
-- Only what the USER enters is stored. Calculated values (early/late start
-- and finish, slack, critical, costs) are NOT stored: the deterministic engine
-- recalculates them every time the file is opened (book section 4.4).
--
-- The outline (WBS) is stored as a tree: each task points to its parent
-- (summary task). `position` is the order among tasks with the same parent.
-- The row number you see on screen ("task 2") is calculated from the tree.
--
--   parent_id  position  name                     on screen
--   NULL       1         Build the shed (summary)   1
--   <shed>     1           Design the shed          2
--   <shed>     2           Get building permit      3
CREATE TABLE tasks (
    id               TEXT PRIMARY KEY NOT NULL,   -- UUID
    -- Summary task this task belongs to, or NULL for a top-level task.
    -- ON DELETE CASCADE: deleting a summary task deletes its subtasks.
    parent_id        TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    position         INTEGER NOT NULL,            -- order among siblings, 1, 2, 3...
    name             TEXT NOT NULL DEFAULT '',
    -- 'auto': the engine calculates the dates. 'manual': the user's dates
    -- (manual_start / manual_finish) are kept as typed (book section 2.3).
    mode             TEXT NOT NULL DEFAULT 'auto' CHECK (mode IN ('auto', 'manual')),
    -- 1 = this task is a milestone (a diamond on the Gantt chart).
    is_milestone     INTEGER NOT NULL DEFAULT 0 CHECK (is_milestone IN (0, 1)),
    -- Duration in working minutes (see the header). NULL = not entered yet.
    duration_minutes INTEGER CHECK (duration_minutes IS NULL OR duration_minutes >= 0),
    -- How the user typed the duration, for display only:
    -- 'm' minutes, 'h' hours, 'd' days, 'w' weeks, 'mo' months;
    -- an 'e' in front means elapsed time (calendar time, e.g. 'ed').
    duration_unit    TEXT NOT NULL DEFAULT 'd'
                     CHECK (duration_unit IN ('m','h','d','w','mo','em','eh','ed','ew','emo')),
    -- Which value stays fixed when another one changes (plan.md §4.2).
    task_type        TEXT NOT NULL DEFAULT 'fixed_units'
                     CHECK (task_type IN ('fixed_units', 'fixed_duration', 'fixed_work')),
    effort_driven    INTEGER NOT NULL DEFAULT 0 CHECK (effort_driven IN (0, 1)),
    -- Scheduling constraint (plan.md §4.2). asap = As Soon As Possible,
    -- alap = As Late As Possible, snet = Start No Earlier Than, snlt = Start
    -- No Later Than, fnet / fnlt = Finish No Earlier / Later Than,
    -- mso / mfo = Must Start On / Must Finish On.
    constraint_type  TEXT NOT NULL DEFAULT 'asap'
                     CHECK (constraint_type IN ('asap','alap','snet','snlt','fnet','fnlt','mso','mfo')),
    -- date-time of the constraint; required by every type except asap/alap.
    constraint_date  TEXT,
    deadline         TEXT,                        -- date-time, or NULL
    manual_start     TEXT,                        -- date-time, manual tasks only
    manual_finish    TEXT,                        -- date-time, manual tasks only
    -- Task calendar, or NULL to use the project calendar.
    calendar_id      TEXT REFERENCES calendars(id) ON DELETE SET NULL,
    notes            TEXT NOT NULL DEFAULT '',
    hyperlink        TEXT NOT NULL DEFAULT '',
    CHECK (parent_id IS NULL OR parent_id <> id),
    CHECK ((constraint_type IN ('asap', 'alap')) = (constraint_date IS NULL))
) STRICT;

-- Finds the children of a summary task quickly, already in order.
CREATE INDEX idx_tasks_parent_position ON tasks(parent_id, position);


-- -----------------------------------------------------------------------------
-- dependencies: links between tasks (book section 2.3).
-- -----------------------------------------------------------------------------
-- "predecessor must finish before successor can start" (type FS), etc.
CREATE TABLE dependencies (
    id             TEXT PRIMARY KEY NOT NULL,   -- UUID of the link itself
    -- Both are foreign keys to tasks.id. Deleting a task deletes its links.
    predecessor_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    successor_id   TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- FS finish-to-start, SS start-to-start, FF finish-to-finish,
    -- SF start-to-finish.
    type           TEXT NOT NULL DEFAULT 'FS' CHECK (type IN ('FS', 'SS', 'FF', 'SF')),
    -- Lag as working minutes; negative = lead (overlap). Used when
    -- lag_percent is NULL.
    lag_minutes    INTEGER NOT NULL DEFAULT 0,
    -- Lag as a fraction of the predecessor's duration (0.5 = 50 %), or NULL.
    lag_percent    REAL,
    -- How the user typed the lag, for display only (same codes as
    -- tasks.duration_unit, plus '%').
    lag_unit       TEXT NOT NULL DEFAULT 'd'
                   CHECK (lag_unit IN ('m','h','d','w','mo','em','eh','ed','ew','emo','%')),
    CHECK (predecessor_id <> successor_id),         -- a task cannot depend on itself
    CHECK (lag_percent IS NULL OR lag_minutes = 0), -- one kind of lag, not both
    UNIQUE (predecessor_id, successor_id)           -- at most one link per pair (like MS Project)
) STRICT;

-- The UNIQUE rule above already gives a fast search by predecessor_id.
-- This index makes "which links point TO this task?" fast as well.
CREATE INDEX idx_dependencies_successor ON dependencies(successor_id);


-- -----------------------------------------------------------------------------
-- resources: people, materials and costs (book section 2.5).
-- -----------------------------------------------------------------------------
-- M0 placeholder: only the basic columns. Rates, costs and the remaining
-- resource fields are added by a later migration in M2.
CREATE TABLE resources (
    id          TEXT PRIMARY KEY NOT NULL,   -- UUID
    name        TEXT NOT NULL DEFAULT '',    -- e.g. 'Anna', 'Timber'
    kind        TEXT NOT NULL DEFAULT 'work' CHECK (kind IN ('work', 'material', 'cost')),
    -- Maximum units for work resources: 1.0 = one full-time person (100 %).
    -- NULL for material and cost resources.
    max_units   REAL CHECK (max_units IS NULL OR max_units >= 0),
    -- Unit name for material resources, e.g. 'plank', 'litre'.
    material_label TEXT NOT NULL DEFAULT '',
    -- Resource calendar, or NULL to use the project calendar.
    calendar_id TEXT REFERENCES calendars(id) ON DELETE SET NULL
) STRICT;


-- -----------------------------------------------------------------------------
-- assignments: which resource works on which task.
-- -----------------------------------------------------------------------------
-- M0 placeholder, like `resources`. Work, cost and tracking columns come in M2.
CREATE TABLE assignments (
    id          TEXT PRIMARY KEY NOT NULL,   -- UUID
    task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    resource_id TEXT NOT NULL REFERENCES resources(id) ON DELETE CASCADE,
    -- Units for work resources (1.0 = 100 %), quantity for material
    -- resources (50 = 50 planks). Cost resources get their amount in M2.
    units       REAL NOT NULL DEFAULT 1.0 CHECK (units >= 0),
    UNIQUE (task_id, resource_id)            -- a resource is assigned once per task
) STRICT;

CREATE INDEX idx_assignments_resource ON assignments(resource_id);


-- -----------------------------------------------------------------------------
-- journal: the change journal, "who changed what, and when" (plan.md §5).
-- -----------------------------------------------------------------------------
-- One row per changed FIELD of one item ("field-level"). All rows created by
-- one user action share the same batch_id (book section 4.4, "Batches").
--
--   id | timestamp            | author  | entity | entity_id | field            | old  | new    | batch_id
--   1  | 2027-02-15T10:42:07Z | Sam Lee | task   | 0b7d...   | duration_minutes | 2400 | 2880   | b71c...
--
--   (task 2 changed from 5 days = 2400 to 6 days = 2880 working minutes on an
--   8-hour calendar; old and new values are JSON text)
--
-- Why entity_id is NOT a foreign key: it points to different tables
-- depending on `entity`, and the journal must keep records about items that
-- have since been deleted. The rules for what is recorded are owned by
-- backend/crates/document (journal.rs); this table only stores them.
CREATE TABLE journal (
    -- INTEGER PRIMARY KEY makes SQLite number the rows 1, 2, 3...
    -- AUTOINCREMENT guarantees a number is never reused, even after old rows
    -- are deleted by compaction, so "record 57" always means the same record.
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,   -- timestamp (UTC) of the change
    author    TEXT NOT NULL,   -- display name from Settings -> User
    -- Kind of item: 'project', 'task', 'dependency', 'resource',
    -- 'assignment', 'calendar', ...
    entity    TEXT NOT NULL,
    entity_id TEXT NOT NULL,   -- UUID of the changed item
    field     TEXT NOT NULL,   -- e.g. 'duration' (model field name); '*' = whole item created/deleted
    -- Values are JSON text, so numbers, text and NULL survive exactly.
    -- old_value NULL = the item was created; new_value NULL = it was deleted.
    old_value TEXT,
    new_value TEXT,
    batch_id  TEXT NOT NULL    -- UUID shared by all records of one user action
) STRICT;

-- "Show the history of this task" must be fast.
CREATE INDEX idx_journal_entity ON journal(entity, entity_id);
-- "Undo / show this whole action" looks up all records of one batch.
CREATE INDEX idx_journal_batch ON journal(batch_id);
-- Compaction deletes records older than a date.
CREATE INDEX idx_journal_timestamp ON journal(timestamp);


-- -----------------------------------------------------------------------------
-- versions: snapshots of the whole project (plan.md §4.7).
-- -----------------------------------------------------------------------------
-- M0 placeholder: the table exists so the format is ready; the feature
-- (backend/crates/gtra/src/versions.rs) is built in M4, which also decides
-- the exact format of `data`.
CREATE TABLE versions (
    id         TEXT PRIMARY KEY NOT NULL,   -- UUID
    created    TEXT NOT NULL,               -- timestamp (UTC)
    kind       TEXT NOT NULL CHECK (kind IN ('automatic', 'named')),
    name       TEXT,                        -- e.g. 'Sent to client v2'; NULL for automatic
    author     TEXT NOT NULL DEFAULT '',
    -- The last journal record included in this snapshot, so the history can
    -- be shown "between" two versions. NULL if the journal was empty.
    journal_id INTEGER,
    -- The snapshot itself (format decided in M4).
    data       BLOB NOT NULL,
    CHECK ((kind = 'named') = (name IS NOT NULL))
) STRICT;

CREATE INDEX idx_versions_created ON versions(created);
