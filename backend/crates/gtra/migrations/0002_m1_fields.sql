-- =============================================================================
-- 0002_m1_fields.sql: format version 2 (milestone M1)
-- =============================================================================
--
-- Reference page: docs/reference/en/backend/crates/gtra/migrations/0002_m1_fields.md
--
-- WHAT THIS FILE IS
-- The second migration of the .gtra format. It runs on top of 0001_init.sql:
-- for a brand-new file both run one after the other; a file saved by an
-- older Ganttora (format version 1) gets only this one when it is opened.
-- The migration runner (src/schema.rs) runs it inside the same transaction
-- as the version stamp, so a file is either fully upgraded or unchanged.
--
-- WHY IT IS NEEDED
-- In M1 the model's calendars got a second list next to the holidays:
-- "exceptions", date ranges with their own working times (a working
-- Saturday, a short day before Christmas, a summer shutdown). Both lists
-- live in the one table `calendar_exceptions`, so the file must remember
--   1. which list a row belongs to (a holiday or an exception), and
--   2. the ORDER of the exceptions, because when two exceptions cover the
--      same date the first one in the list wins (model::Calendar).
--
-- Almost all other M1 fields already had a column in 0001_init.sql (task
-- mode, constraint, deadline, notes, hyperlink, task calendar, the project's
-- schedule_from and finish_date, percentage lags). Nothing else is added.
--
-- WHAT CHANGES
--
--   calendar_exceptions (before)               calendar_exceptions (after)
--   ┌─────────────┐                            ┌─────────────┐
--   │ id          │                            │ id          │
--   │ calendar_id │                            │ calendar_id │
--   │ name        │                            │ name        │
--   │ start_date  │                            │ start_date  │
--   │ end_date    │                            │ end_date    │
--   │ is_working  │                            │ is_working  │
--   └─────────────┘                            │ kind        │ NEW 'holiday' | 'exception'
--                                              │ position    │ NEW order among exceptions
--                                              └─────────────┘
--
--   kind = 'holiday'    the row is (part of) model::Calendar::holidays.
--                       Always non-working (is_working = 0). A holiday row
--                       may cover several days; every day is a holiday.
--   kind = 'exception'  the row is one entry of model::Calendar::exceptions.
--                       is_working = 1 when it has working times (they are
--                       in calendar_exception_times), 0 when it is a
--                       non-working range.
--   position            1, 2, 3, ... for exceptions, in list order;
--                       0 for holidays (they have no order: a set of dates).
--
-- HOW EXISTING (VERSION 1) DATA IS UPGRADED
-- A version-1 file was written by the M0 code, which knew only holidays:
--   * non-working rows were holidays    -> kind 'holiday' (the default)
--   * working rows were not in the model -> kind 'exception', position 0
--     (reading sorts equal positions by start date, then id)
--   * tasks.is_milestone was not typed by the user: M0 wrote
--     "duration = 0" into it. In M1 it is the user's own "mark as
--     milestone" flag, so it is reset to 0. A zero-duration task is still
--     shown as a milestone (model::Task::is_milestone checks the duration).
--
-- RULES (see 0001_init.sql): this file is never edited once released; the
-- next change is 0003_....sql.
-- =============================================================================


-- Which list of the model calendar the row belongs to. ADD COLUMN puts the
-- DEFAULT value into every existing row, so all old rows start as holidays.
ALTER TABLE calendar_exceptions
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'holiday'
    CHECK (kind IN ('holiday', 'exception'));

-- Order of the exceptions of one calendar (the first one wins on overlap).
ALTER TABLE calendar_exceptions
    ADD COLUMN position INTEGER NOT NULL DEFAULT 0
    CHECK (position >= 0);

-- Working rows were never holidays (a holiday is a day off).
UPDATE calendar_exceptions SET kind = 'exception' WHERE is_working = 1;

-- M0 derived is_milestone from the duration; M1 stores the user's flag.
UPDATE tasks SET is_milestone = 0;
