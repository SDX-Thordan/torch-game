import { defineConfig } from "vite";

export default defineConfig({
  // Relative base so the built assets work inside the Capacitor webview, which
  // serves from a file/asset origin rather than a web server root.
  base: "./",
  build: {
    outDir: "dist",
    target: "es2022",
    sourcemap: true,
  },
});
