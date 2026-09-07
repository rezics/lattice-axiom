import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
    base: "./",
    plugins: [react()],
    build: { target: "es2022", sourcemap: false, assetsInlineLimit: 0 },
    server: { strictPort: true, port: 1420 },
});
