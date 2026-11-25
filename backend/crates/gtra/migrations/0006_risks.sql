-- =============================================================================
-- 0006_risks.sql: format version 6
-- =============================================================================
--
-- Reference page: docs/reference/en/backend/crates/gtra/migrations/0006_risks.md
--
-- WHAT THIS FILE IS
-- The sixth migration of the .gtra format; see 0003_blockers.sql for how
-- migrations run.
--
-- WHAT CHANGES
-- A task can have several risks (model::Risk), each with a priority, a
-- response and a consequence. They get their own table, like blockers; the
-- single text column `tasks.risk` (format 5) is removed.
--
-- HOW EXISTING (VERSION 5) DATA IS UPGRADED
-- Every risk text becomes one row: medium priority, to be mitigated, with
-- the mitigation and consequence still to be written. The task's UUID is
-- used as the risk's UUID: at most one risk per task, so it is unique in
-- this table, and the upgrade always gives the same result.
--
-- RULES (see 0001_init.sql): this file is never edited once released; the
-- next change is 0007_....sql.
-- =============================================================================

CREATE TABLE risks (
    id             TEXT PRIMARY KEY NOT NULL,   -- UUID
    -- The task at risk. Deleting the task deletes its risks.
    task_id        TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- Order in the task's list of risks: 1, 2, 3 ...
    position       INTEGER NOT NULL,
    -- What could go wrong.
    description    TEXT NOT NULL,
    priority       TEXT NOT NULL CHECK (priority IN ('low', 'medium', 'high')),
    response       TEXT NOT NULL CHECK (response IN ('approved', 'mitigate')),
    -- How it is mitigated; '' for an approved risk.
    mitigation     TEXT NOT NULL DEFAULT '',
    -- 1 = the user ticked "no consequence".
    no_consequence INTEGER NOT NULL DEFAULT 0 CHECK (no_consequence IN (0, 1)),
    consequence    TEXT NOT NULL DEFAULT ''
) STRICT;

CREATE INDEX idx_risks_task ON risks(task_id, position);

INSERT INTO risks (id, task_id, position, description, priority, response)
SELECT id, id, 1, risk, 'medium', 'mitigate' FROM tasks WHERE trim(risk) <> '';

ALTER TABLE tasks DROP COLUMN risk;
