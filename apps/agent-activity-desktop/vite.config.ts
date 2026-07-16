import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const configuredPort = Number.parseInt(process.env.AGENT_ACTIVITY_DEV_PORT ?? "1420", 10);
const devPort = Number.isInteger(configuredPort) && configuredPort > 0 && configuredPort <= 65_535
  ? configuredPort
  : 1420;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: devPort,
    strictPort: true,
    host: "127.0.0.1",
  },
});
