import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
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
