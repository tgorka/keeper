import path from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}", "scripts/**/*.test.ts"],
    // This exists to keep the two budgets ordered, not to buy time. Vitest's
    // default test timeout is 5 s and `src/test/setup.ts` gives Testing Library
    // a 5 s `asyncUtilTimeout` — the same number, so a `waitFor` that spends its
    // whole budget is killed at the instant it would have reported which
    // element it could not find. The test dies with someone else's message, or
    // none. A per-test budget must exceed the per-wait budget for a wait to be
    // able to fail on its own terms.
    //
    // Three times the wait budget, and it weakens no assertion: it only
    // lengthens the worst case for a test that was going to fail anyway.
    testTimeout: 15_000,
    coverage: {
      provider: "v8",
      include: ["src/**"],
      exclude: ["src/components/ui/**", "src/test/**"],
    },
  },
});
