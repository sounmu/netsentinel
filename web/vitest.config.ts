import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

// Mirror the `@/*` path alias from tsconfig.json so component tests can
// import modules the same way the app does.
export default defineConfig({
  resolve: {
    alias: { "@": fileURLToPath(new URL(".", import.meta.url)) },
  },
});
