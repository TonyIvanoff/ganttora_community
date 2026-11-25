-- =============================================================================
-- 0007_plan.sql: format version 7
-- =============================================================================
--
-- Reference page: docs/reference/en/backend/crates/gtra/migrations/0007_plan.md
--
-- WHAT THIS FILE IS
-- The seventh migration of the .gtra format; see 0003_blockers.sql for how
-- migrations run.
--
-- WHAT CHANGES
-- Two date-times per task (plan.md D30):
--   planned_finish   the finish saved by "Set plan"
--   actual_finish    when the work really finished
--
-- HOW EXISTING (VERSION 6) DATA IS UPGRADED
-- Both are NULL: no plan was saved, and the real finish is not known.
--
-- RULES (see 0001_init.sql): this file is never edited once released; the
-- next change is 0008_....sql.
-- =============================================================================

ALTER TABLE tasks ADD COLUMN planned_finish TEXT;  -- date-time, or NULL
ALTER TABLE tasks ADD COLUMN actual_finish TEXT;   -- date-time, or NULL
