"use strict";
let port;
const status = document.getElementById("status");
window.addEventListener("message", (event) => {
  if (event.data !== "rem-plugin-config-v1" || !event.ports[0]) return;
  port = event.ports[0];
  port.onmessage = (message) => {
    try {
      const value = JSON.parse(message.data);
      if (value.type === "state") {
        status.textContent = value.status;
        document.getElementById("state").textContent = JSON.stringify(value, null, 2);
      }
      if (value.type === "validationError") status.textContent = value.message;
    } catch (_) { status.textContent = "Malformed configuration response"; }
  };
  port.start();
  port.postMessage(JSON.stringify({ type: "getState" }));
});
document.getElementById("open").addEventListener("click", () => {
  if (port) port.postMessage(JSON.stringify({ type: "action", action: "settings.open" }));
});
