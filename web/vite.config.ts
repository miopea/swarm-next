import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 8766,
    strictPort: true,
    proxy: {
      "/api": { target: "http://127.0.0.1:8765", ws: true },
      "/health": "http://127.0.0.1:8765",
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
    // Vitest's 5s default assumes the async utilities below it finish well
    // inside it. They are now allowed 5s of their own, so a test that waits
    // has to be able to outlast one.
    testTimeout: 20_000,
  },
});
