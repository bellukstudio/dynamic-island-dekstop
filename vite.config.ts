import { defineConfig, loadEnv } from "vite";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { resolve } from "path";
// Host khusus untuk pengembangan Tauri
const host = process.env.TAURI_DEV_HOST;

// Lokasi root project, yaitu folder yang berisi package.json
const projectRoot = fileURLToPath(new URL(".", import.meta.url));

export default defineConfig(({ mode }) => {
  // Ganti parameter ketiga jadi "" agar membaca semua variabel tanpa filter
  const env = loadEnv(mode, projectRoot, "");

  // Pemeriksaan konfigurasi di Terminal Node.js
  console.info("[Vite env check]", {
    mode,
    projectRoot,
    projectIdPresent: Boolean(env.VITE_FIREBASE_PROJECT_ID?.trim()),
    apiKeyPresent: Boolean(env.VITE_FIREBASE_API_KEY?.trim()),
  });

  return {
    root: projectRoot,
    envDir: projectRoot,
    envPrefix: ["VITE_", "TAURI_ENV_*"],

    // 👇 TAMBAHKAN BAGIAN INI (Agar nilainya diteruskan ke kode frontend Anda)
    define: {
      "import.meta.env.VITE_FIREBASE_PROJECT_ID": JSON.stringify(
        env.VITE_FIREBASE_PROJECT_ID || ""
      ),
      "import.meta.env.VITE_FIREBASE_API_KEY": JSON.stringify(
        env.VITE_FIREBASE_API_KEY || ""
      ),
    },

    clearScreen: false,

    server: {
      port: 1420,
      strictPort: true,
      host: host || false,
      hmr: host
        ? {
          protocol: "ws",
          host,
          port: 1421,
        }
        : undefined,
      watch: {
        ignored: ["**/src-tauri/**"],
      },
    },

    build: {
      rollupOptions: {
        input: {
          main: resolve(import.meta.dirname, "index.html"),
          settings: resolve(import.meta.dirname, "settings.html"),
          "note-detail": resolve(import.meta.dirname, "note-detail.html"),
          "compress": resolve(import.meta.dirname, "compress.html"),
          "convert": resolve(import.meta.dirname, "convert.html"),
          "share": resolve(import.meta.dirname, "share.html"),
          "android": resolve(import.meta.dirname, "android.html"),
          "whatsapp": resolve(import.meta.dirname, "whatsapp.html"),
          "whats-new":  resolve(import.meta.dirname, "whats-new.html"),
        },
      },
    },
  };
});