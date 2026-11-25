/**
 * mockSchedule.ts: makes a large, fake schedule for the M0 Gantt spike.
 *
 * What it is for:
 *   Milestone M0 must prove that the Gantt chart scrolls smoothly with
 *   10,000 tasks. The garden shed has only 8 tasks, so
 *   this file invents tasks that *look like* a schedule result from Rust:
 *   an id, a name, start and finish dates, a "critical" flag and a list of
 *   predecessors.
 *
 * Where it fits:
 *   Called by the store (`state/store.ts`, `setShowMock`) the first time the
 *   mock is shown: when the user presses "Demo: 10,000 tasks", or at
 *   start-up in a plain browser where there is no Rust side. The result is
 *   kept and reused. The Gantt view only sees the tasks; it does not know
 *   they are fake. Real projects come from the Rust engine
 *   (`features/gantt/fromSnapshot.ts`); this file is only for demos and
 *   performance tests.
 *
 * Main ideas:
 *   - **Deterministic:** the same `seed` always gives exactly the same tasks
 *     (same ids, same dates). Tests and performance measurements can then be
 *     repeated. We use our own tiny random-number generator because
 *     `Math.random()` cannot be given a seed.
 *   - **Not scheduling:** the dates here are *invented* with a few simple
 *     rules so that the links look sensible. This is not the scheduling
 *     engine, and nothing in the real UI may compute dates (decision D16).
 *     The `isCritical` flag is random, too.
 *   - Dates are "floating" local date-times without a time zone, written as
 *     ISO 8601 text: "2027-01-04T08:00:00". The Rust engine uses the same
 *     form (chrono `NaiveDateTime`).
 */

/**
 * One task as the Gantt view receives it: the same shape a schedule result
 * from Rust will have (field names in camelCase, as serde will send them).
 */
export interface MockScheduleTask {
  /** Unique id: a UUID (version 4) written as text. Never a row number. */
  id: string;
  /** Task name shown next to the bar. */
  name: string;
  /** Start date-time, ISO 8601 without time zone, e.g. "2027-01-04T08:00:00". */
  start: string;
  /** Finish date-time, ISO 8601 without time zone, e.g. "2027-01-08T17:00:00". */
  finish: string;
  /** `true` when the task is on the critical path (drawn with a dark outline). */
  isCritical: boolean;
  /** Ids of the tasks this task depends on (finish-to-start links). */
  predecessors: string[];
}

/** Options for {@link generateMockSchedule}. */
export interface MockScheduleOptions {
  /** How many tasks to create, e.g. 10_000. */
  count: number;
  /** Seed for the random generator. Same seed = same schedule. */
  seed: number;
  /**
   * Makes the display name of task number `n` (1-based). The caller passes a
   * function that uses the translation file, so no text is hard-coded here.
   */
  nameFor: (n: number) => string;
}

/** Milliseconds in one day. */
const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * The fake project starts on Monday 4 January 2027.
 * `Date.UTC` gives milliseconds since 1 Jan 1970. We use UTC for all date
 * math so that daylight-saving changes never shift a date by an hour.
 */
const PROJECT_START_MS = Date.UTC(2027, 0, 4);

/** Tasks are grouped in phases of this many tasks (like summary groups). */
const PHASE_SIZE = 50;

/** Each new phase starts this many working days after the previous one. */
const PHASE_STEP_WORKDAYS = 3;

/**
 * Creates a seeded pseudo-random number generator ("mulberry32").
 *
 * @param seed - Any whole number. The same seed gives the same sequence.
 * @returns A function; each call returns the next number in [0, 1).
 *
 * @example
 * const rnd = createRandom(42);
 * rnd(); // always the same first value for seed 42
 *
 * Beginner note: the returned function "remembers" the variable `state`
 * even after `createRandom` has finished. Such a function is called a
 * *closure*. The `>>>`, `^` and `Math.imul` operations mix the bits of the
 * number so the output looks random.
 */
export function createRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Makes a UUID version 4 string from the seeded generator.
 *
 * @param rnd - The random generator from {@link createRandom}.
 * @returns Text like "3f2b8c1e-9a4d-4e2f-8b1c-0d9e7f6a5b4c".
 *
 * A real app uses `crypto.randomUUID()`, which cannot be seeded. Here we
 * need repeatable ids, so we build the 16 random bytes ourselves and set the
 * "version" and "variant" bits the UUID standard (RFC 9562) requires.
 */
export function seededUuid(rnd: () => number): string {
  const bytes: number[] = [];
  for (let i = 0; i < 16; i++) bytes.push(Math.floor(rnd() * 256));
  // Byte 6: high 4 bits = 0100 (version 4). Byte 8: high 2 bits = 10 (variant).
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x40;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex = bytes.map((b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/**
 * Turns a working-day number into a calendar date, skipping weekends.
 *
 * @param workday - 0 = the project start (a Monday), 1 = Tuesday, …,
 *   5 = the next Monday.
 * @returns Milliseconds (UTC) of midnight on that calendar day.
 *
 * @example
 * workdayToDayMs(5) === PROJECT_START_MS + 7 * DAY_MS; // next Monday
 */
function workdayToDayMs(workday: number): number {
  const weeks = Math.floor(workday / 5);
  const weekday = workday % 5;
  return PROJECT_START_MS + (weeks * 7 + weekday) * DAY_MS;
}

/**
 * Formats UTC milliseconds as a time-zone-free ISO date-time.
 *
 * @param ms - Milliseconds since 1970 (UTC).
 * @returns Text like "2027-01-04T08:00:00".
 */
function toIsoLocal(ms: number): string {
  // toISOString() gives "2027-01-04T08:00:00.000Z"; drop ".000Z".
  return new Date(ms).toISOString().slice(0, 19);
}

/**
 * Generates a deterministic fake schedule.
 *
 * @param options - Count, seed and the name function; see {@link MockScheduleOptions}.
 * @returns An array of `count` tasks, in display order.
 *
 * Rules used to invent the dates (fake, see the file header):
 * - Tasks come in phases of 50. Each phase starts 3 working days after the
 *   previous one, so 10,000 tasks cover about 2½ years.
 * - Each task lasts 1–10 working days, 08:00 to 17:00.
 * - About 60% of tasks get one or two predecessors among the 1–5 tasks just
 *   above them (in the same phase). Such a task starts on the working day
 *   after its latest predecessor finishes; links then point forward in time.
 * - About 12% of tasks are flagged critical.
 *
 * @example
 * const tasks = generateMockSchedule({ count: 3, seed: 1, nameFor: (n) => `T${n}` });
 * tasks.length; // 3
 */
export function generateMockSchedule(options: MockScheduleOptions): MockScheduleTask[] {
  const { count, seed, nameFor } = options;
  const rnd = createRandom(seed);
  const tasks: MockScheduleTask[] = [];
  // Last working day of each task, kept so successors can start after it.
  const lastWorkday: number[] = [];

  for (let i = 0; i < count; i++) {
    const phase = Math.floor(i / PHASE_SIZE);
    const indexInPhase = i % PHASE_SIZE;
    const phaseStart = phase * PHASE_STEP_WORKDAYS;

    // Without predecessors: start somewhere in the first 10 days of the phase.
    let firstWorkday = phaseStart + Math.floor(rnd() * 10);
    const predecessors: string[] = [];

    if (indexInPhase > 0 && rnd() < 0.6) {
      const linkCount = rnd() < 0.25 ? 2 : 1;
      for (let k = 0; k < linkCount; k++) {
        // A task 1..5 rows above, but not before the start of this phase.
        const back = 1 + Math.floor(rnd() * Math.min(5, indexInPhase));
        const predIndex = i - back;
        const pred = tasks[predIndex];
        const predLast = lastWorkday[predIndex];
        if (pred === undefined || predLast === undefined) continue;
        if (predecessors.includes(pred.id)) continue;
        predecessors.push(pred.id);
        firstWorkday = Math.max(firstWorkday, predLast + 1);
      }
    }

    const durationDays = 1 + Math.floor(rnd() * 10);
    const last = firstWorkday + durationDays - 1;
    lastWorkday.push(last);

    tasks.push({
      id: seededUuid(rnd),
      name: nameFor(i + 1),
      // 08:00 on the first day, 17:00 on the last day.
      start: toIsoLocal(workdayToDayMs(firstWorkday) + 8 * 60 * 60 * 1000),
      finish: toIsoLocal(workdayToDayMs(last) + 17 * 60 * 60 * 1000),
      isCritical: rnd() < 0.12,
      predecessors,
    });
  }
  return tasks;
}
