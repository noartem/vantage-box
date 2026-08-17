import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig({
  plugins: [sveltekit()],

  // Web-воркеры собираем ES-модулями: бандл воркера (schema-lint-worker) тянет
  // json-schema-library и @codemirror/lint, из-за чего получает code-splitting,
  // а дефолтный формат `iife` его не поддерживает. ES-формат поддерживает, и
  // Chromium-вебвью Tauri умеет module-воркеры.
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
    // Явный IPv4: по умолчанию Vite слушает только [::1], и тогда devUrl
    // http://127.0.0.1:1420 не открывается, а окно приложения остаётся пустым.
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
