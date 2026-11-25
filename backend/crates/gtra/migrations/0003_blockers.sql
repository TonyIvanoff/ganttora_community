-- =============================================================================
-- 0003_blockers.sql: format version 3 (blockers, plan.md D23)
-- =============================================================================
--
-- Reference page: docs/reference/en/backend/crates/gtra/migrations/0003_blockers.md
--
-- WHAT THIS FILE IS
-- The third migration of the .gtra format. It runs after 0001_init.sql and
-- 0002_m1_fields.sql: a brand-new file gets all three; a file saved by an
-- older Ganttora (format version 1 or 2) gets the ones it is missing when it
-- is opened. The migration runner (src/schema.rs) runs them in one
-- transaction together with the version stamp, so a file is either fully
-- upgraded or unchanged.
--
-- WHY IT IS NEEDED
-- A task can now carry "blockers" (model::Blocker): unplanned delays such as
-- "the wood delivery is one week late". Each blocker says: after this much of
-- the task's work was done, the task was stuck for this long, because of
-- this reason. Until now the file had no place for them, so they were lost
-- on save.
--
-- WHAT CHANGES: one new table
--
--   tasks                              blockers (NEW)
--   ┌──────────────┐                   ┌────────────────────┐
--   │ id (PK)      │◄──────────────────│ task_id (FK)       │ the task that is blocked
--   │ ...          │   1 task :        │ id (PK)            │ UUID of the blocker
--   └──────────────┘   many blockers   │ position           │ 1, 2, 3 ... order in Task::blockers
--                                      │ after_work_minutes │ work done before the delay
--                                      │ duration_minutes   │ length of the delay
--                                      │ reason             │ free text ('' = none)
--                                      │ reported_on        │ date '2027-03-04', or NULL
--                                      └────────────────────┘
--
-- Example: the shed's "Buy materials" (3 days of work) is stuck for 5 working
-- days after 2 days of work, because the wood comes late:
--
--   id      task_id   position  after_work_minutes  duration_minutes  reason              reported_on
--   'b1…'   'a7…'     1         960                 2400              'Wood delivery late' '2027-03-04'
--
-- COLUMN NOTES
--   position            The model keeps blockers in a list (Vec). The engine
--                       sorts them by after_work itself, but the file keeps
--                       the list order so a save and a reopen give back
--                       exactly the same list.
--   after_work_minutes  Working minutes (like every duration in the file, see
--   duration_minutes    0001_init.sql). Deliberately NO "CHECK (>= 0)": the
--                       app never writes negative values (the document crate
--                       refuses them), but if another tool does, the row is
--                       still loaded and the user is told (read.rs adds a
--                       ReadNote) instead of the whole file failing to open.
--   reported_on         The day the user heard about the delay. Information
--                       only; the engine ignores it.
--
-- ON DELETE CASCADE: deleting a task deletes its blockers with it.
--
-- HOW EXISTING (VERSION 2) DATA IS UPGRADED
-- Nothing to convert: older files had no blockers, so the new table simply
-- starts empty.
--
-- RULES (see 0001_init.sql): this file is never edited once released; the
-- next change is 0004_....sql.
-- =============================================================================


CREATE TABLE blockers (
    id                 TEXT PRIMARY KEY NOT NULL,   -- UUID
    -- The blocked task. Deleting the task deletes its blockers.
    task_id            TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- Order in the task's list of blockers: 1, 2, 3 ...
    position           INTEGER NOT NULL,
    -- Working minutes of the task's own work done before the delay starts.
    after_work_minutes INTEGER NOT NULL,
    -- How long the task is blocked, in working minutes (task calendar).
    duration_minutes   INTEGER NOT NULL,
    -- Why the task is blocked; '' when no reason was given.
    reason             TEXT NOT NULL DEFAULT '',
    -- Date the delay was reported ('2027-03-04'), or NULL if not given.
    reported_on        TEXT
) STRICT;

-- Reading loads the blockers task by task in list order; this index makes
-- "all blockers of task X, by position" fast.
CREATE INDEX idx_blockers_task ON blockers(task_id, position);
