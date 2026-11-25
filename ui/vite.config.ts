import { defineConfig } from "vite";
import preact from "@preact/preset-vite";
import tailwindcss from "@tailwindcss/vite";

// The UI talks to the engine over a loopback WebSocket. In dev the engine runs
// on the host; in the Tauri shell it is spawned by the backend.
// There is no default: each shell injects its own engine URL into its window
// (window.__PEDAL_ENGINE_URL__). VITE_ENGINE_URL is only for pointing a plain
// browser tab at one specific engine (e.g. `pedal-engine --headless --port N`).
const ENGINE_URL = process.env.VITE_ENGINE_URL;

export default defineConfig({
  plugins: [preact(), tailwindcss()],
  // `base: "./"` so the built assets load from the embedded Tauri protocol and
  // from a plain file:// open during development.
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        main: "index.html",
        demo: "src/demo/index.html",
      },
    },
  },
  server: {
    port: 5173,
    // Proxy the engine socket so the dev server and the app share an origin.
    proxy: ENGINE_URL
      ? { "/engine": { target: ENGINE_URL, ws: true, changeOrigin: true } }
      : undefined,
  },
  define: {
    __ENGINE_URL__: JSON.stringify(ENGINE_URL ?? "__ENGINE_URL__"),
  },
});
