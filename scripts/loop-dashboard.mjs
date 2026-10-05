#!/usr/bin/env node
// Loop dashboard — redesigned (see scripts/dashboard/).
// Thin entry point: wires paths + port, starts the server.
// Launch stays `node scripts/loop-dashboard.mjs`.
import { createDashboardServer } from "./dashboard/server/http.mjs";

const PORT = Number(process.env.GYRE_DASHBOARD_PORT || 7690);

createDashboardServer({
  port: PORT,
  root: process.cwd(),
  here: new URL(".", import.meta.url).pathname,
}).listen(PORT, "127.0.0.1", () => {
  console.log(`loop dashboard: http://127.0.0.1:${PORT}`);
});
