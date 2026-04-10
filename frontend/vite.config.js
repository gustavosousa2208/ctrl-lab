import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
var host = process.env.TAURI_DEV_HOST;
export default defineConfig({
    clearScreen: false,
    plugins: [react()],
    server: {
        port: 5173,
        strictPort: true,
        host: host || false,
        hmr: host
            ? {
                protocol: "ws",
                host: host,
                port: 1421,
            }
            : undefined,
        watch: {
            ignored: ["**/src-tauri/**"],
        },
    },
    envPrefix: ["VITE_", "TAURI_ENV_*"],
    build: {
        target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari15",
        minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
        sourcemap: !!process.env.TAURI_ENV_DEBUG,
    },
});
