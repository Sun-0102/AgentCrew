import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    // Rust sources are watched by Tauri; Vite must not watch locked build outputs.
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
