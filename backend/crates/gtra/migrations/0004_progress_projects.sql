-- =============================================================================
-- 0004_progress_projects.sql: format version 4 (plan.md D25)
-- =============================================================================
--
-- Reference page: docs/reference/en/backend/crates/gtra/migrations/0004_progress_projects.md
--
-- WHAT THIS FILE IS
-- The fourth migration of the .gtra format; see 0003_blockers.sql for how
-- migrations run.
--
-- WHAT CHANGES: two new columns in the table `tasks`
--   percent_complete   how much of the task is done, 0 to 100
--   is_project         1 = the row is a project: a top-level row that holds
--                      tasks. One file can hold several.
--
-- HOW EXISTING (VERSION 3) DATA IS UPGRADED
-- Every task starts at 0 % and none is a project. The app then adds one
-- project row above the old tasks when it opens the file
-- (model::Project::ensure_project_row).
--
-- RULES (see 0001_init.sql): this file is never edited once released; the
-- next change is 0005_....sql.
-- =============================================================================

ALTER TABLE tasks
    ADD COLUMN percent_complete INTEGER NOT NULL DEFAULT 0
    CHECK (percent_complete BETWEEN 0 AND 100);

ALTER TABLE tasks
    ADD COLUMN is_project INTEGER NOT NULL DEFAULT 0
    CHECK (is_project IN (0, 1));
