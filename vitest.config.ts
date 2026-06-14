import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["test/**/*.test.ts"],
    // The economy stability test sweeps many seeds across tens of thousands of
    // ticks; give it room without being slow on the fast unit tests.
    testTimeout: 60_000,
  },
});
