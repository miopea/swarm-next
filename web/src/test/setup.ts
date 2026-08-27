import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach } from "vitest";

// Unmount rendered components between tests, for every file.
//
// Vitest runs without `globals`, so Testing Library never installs its own
// auto-cleanup, and five test files did not install one either. Their tests
// were querying a document that still held the previous test's render, which
// is why the suite failed differently depending on the order it ran in.
//
// Files that already call `afterEach(cleanup)` themselves are unaffected:
// cleanup on an empty document does nothing.
afterEach(cleanup);

// Testing Library's asyncUtilTimeout defaults to 1000ms, and that default
// assumes a machine with nothing else on it. This one never is: two workers
// building at once is the ordinary state of this Hive, and Queen recorded 26
// concurrent build processes in a single review cycle.
//
// MEASURED, not guessed. App.test.tsx passes at 300ms and fails at 200ms, so
// its slowest assertions settle in 200-300ms idle -- three to five times inside
// the old limit, not the comfortable margin the default implies. Under 26 CPU
// burners on this 4-core box the same file failed 2 of 26 at the default,
// naming the same "Unable to find role=heading" error the operator's runs hit.
//
// 5000ms is ~20x the measured idle worst case. It does not make a broken
// assertion pass; it stops a SLOW one being reported as a broken one. The cost
// is that a genuine failure takes 5s rather than 1s to report, which is the
// right trade against a suite that trains its readers to re-run rather than
// to read.
configure({ asyncUtilTimeout: 5_000 });
