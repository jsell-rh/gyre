#!/usr/bin/env node
// Durable controller cockpit. Launch stays `node scripts/loop-dashboard.mjs`.
import { createDashboardServer } from "./dashboard/server/dev-http.mjs";

const PORT = Number(process.env.GYRE_DASHBOARD_PORT || 7690);

createDashboardServer({
  port: PORT,
  root: process.cwd(),
  here: new URL(".", import.meta.url).pathname,
}).listen(PORT, "127.0.0.1", () => {
  console.log(`loop dashboard: http://127.0.0.1:${PORT}`);
});
