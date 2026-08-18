import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { paraglideVitePlugin } from "@inlang/paraglide-js";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    sveltekit(),
    // Compile-time, type-safe i18n. We drive the locale ourselves from
    // src/lib/i18n.svelte.ts (system auto-detection + localStorage preference),
    // so the strategy stays on baseLocale and Paraglide never overrides the
    // user's "system follows OS" choice.
    paraglideVitePlugin({
      project: "./project.inlang",
      outdir: "./src/lib/paraglide",
      emitTsDeclarations: true,
      // globalVariable lets setLocale(code, { reload: false }) switch the
      // in-memory locale that m.x() reads; baseLocale is the fallback before
      // applyLocale() runs. No url/localStorage strategies: this is a desktop
      // app with no URL bar, and we persist the preference ourselves.
      strategy: ["globalVariable", "baseLocale"],
    }),
  ],

  // Web workers are bundled as ES modules: the schema-lint worker bundle pulls
  // json-schema-library and @codemirror/lint, which triggers code-splitting, and
  // the default `iife` format does not support that. ES does, and the Tauri
  // Chromium webview supports module workers.
  worker: {
    format: 'es'
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    // Explicit IPv4: by default Vite listens only on [::1], so the devUrl
    // http://127.0.0.1:1420 would not open and the app window would stay blank.
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
});