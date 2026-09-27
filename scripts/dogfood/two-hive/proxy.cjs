#!/usr/bin/env node
// A byte-level proxy standing where a tunnel stands in front of a Keeper.
//
// ⚠️ IT ANSWERS 502 WHILE THE KEEPER IS DOWN, WHICH A PLAIN PORT DOES NOT. A
// member talking straight to a restarting Keeper sees "connection refused"; one
// behind a tunnel or reverse proxy sees an HTTP 502. The field runs behind one.
//
// It can also refuse one path with a chosen status while FAULT_FILE exists —
// the file holds "PATH_PREFIX STATUS", e.g. "/api/v1/federation/catalog 409".
// That is how the acceptance run shows a member surviving a refused request,
// the failure that silenced a field member for days.
const fs = require("node:fs");
const net = require("node:net");

const [listenPort, upstreamPort] = process.argv.slice(2, 4).map(Number);
const faultFile = process.argv[4];
if (!listenPort || !upstreamPort) {
  console.error("usage: proxy.cjs LISTEN_PORT UPSTREAM_PORT [FAULT_FILE]");
  process.exit(2);
}

/** The injected status for a request in this chunk, if a fault covers it. */
function injectedFault(chunk) {
  if (!faultFile || !fs.existsSync(faultFile)) return undefined;
  const [prefix, status] = fs.readFileSync(faultFile, "utf8").trim().split(/\s+/);
  // Every request line in the chunk, not only the first on the connection: the
  // member's HTTP client reuses connections, so a later request would slip by.
  for (const line of chunk.toString("latin1").split("\r\n")) {
    const match = /^[A-Z]+ (\S+) HTTP\/1\.[01]$/.exec(line);
    if (match && match[1].startsWith(prefix)) return Number(status);
  }
  return undefined;
}

net.createServer((client) => {
  const upstream = net.connect({ host: "127.0.0.1", port: upstreamPort });
  let connected = false;
  const pending = [];
  const refuse = (status) => {
    client.end(`HTTP/1.1 ${status} Injected\r\ncontent-length: 0\r\nconnection: close\r\n\r\n`);
    upstream.destroy();
  };
  client.on("data", (chunk) => {
    const status = injectedFault(chunk);
    if (status) return refuse(status);
    if (connected) upstream.write(chunk);
    else pending.push(chunk);
  });
  upstream.once("connect", () => {
    connected = true;
    for (const chunk of pending.splice(0)) upstream.write(chunk);
    upstream.pipe(client);
  });
  upstream.on("error", () => {
    if (!connected) {
      client.end("HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
    } else {
      client.destroy();
    }
  });
  client.on("error", () => upstream.destroy());
  client.on("close", () => upstream.destroy());
  upstream.on("close", () => client.destroy());
}).listen(listenPort, "127.0.0.1", () => {
  console.log(`proxy 127.0.0.1:${listenPort} -> 127.0.0.1:${upstreamPort}`);
});
