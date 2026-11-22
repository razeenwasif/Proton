import test from "node:test";
import assert from "node:assert/strict";
import { parseSshCommand, remoteSummary, renderRemoteModalHtml } from "../../ui/remote.js";

test("parseSshCommand parses modal open, host, folder, window flag, forward, and disconnect", () => {
  assert.deepEqual(parseSshCommand("ssh"), { kind: "open" });
  assert.deepEqual(parseSshCommand("remote"), { kind: "open" });
  assert.deepEqual(parseSshCommand("ssh mini"), {
    kind: "connect",
    host: "mini",
    folder: null,
    newWindow: false,
  });
  assert.deepEqual(parseSshCommand("ssh mini:~/Github/Applications/Proton"), {
    kind: "connect",
    host: "mini",
    folder: "~/Github/Applications/Proton",
    newWindow: false,
  });
  assert.deepEqual(parseSshCommand("ssh -w mini ~/Github/Applications/Graviton"), {
    kind: "connect",
    host: "mini",
    folder: "~/Github/Applications/Graviton",
    newWindow: true,
  });
  assert.deepEqual(parseSshCommand("ssh disconnect"), { kind: "disconnect" });
  assert.deepEqual(parseSshCommand("ssh local"), { kind: "disconnect" });
  assert.deepEqual(parseSshCommand("ssh forward 8080:3000"), {
    kind: "forward",
    localPort: 8080,
    remotePort: 3000,
  });
  assert.deepEqual(parseSshCommand("ssh forward 5173"), {
    kind: "forward",
    localPort: 5173,
    remotePort: 5173,
  });
  assert.equal(parseSshCommand("w"), null);
});

test("remoteSummary reflects local vs connected vs disconnected error states", () => {
  const local = remoteSummary(null);
  assert.equal(local.connected, false);
  assert.equal(local.label, "Local");

  const conn = remoteSummary({
    connected: true,
    host: "mini",
    folder: "/Users/razeenwasif/Github/Applications/Proton",
    remoteHostname: "Kagutsuchi-2.local",
    remoteHome: "/Users/razeenwasif",
    error: null,
    forwardedPorts: [{ localPort: 18091, remotePort: 18091, auto: true }],
  });
  assert.equal(conn.connected, true);
  assert.equal(conn.label, "SSH: mini");
  assert.equal(conn.tone, "ok");

  const err = remoteSummary({
    connected: false,
    host: "mini",
    folder: null,
    remoteHostname: null,
    remoteHome: null,
    error: "Connection reset",
    forwardedPorts: [],
  });
  assert.equal(err.tone, "err");
});

test("renderRemoteModalHtml renders hosts, repo directories, and active tunnels safely", () => {
  const html = renderRemoteModalHtml({
    status: {
      connected: true,
      host: "mini",
      folder: "/Users/razeenwasif/Github/Applications/Proton",
      remoteHostname: "Kagutsuchi-2.local",
      remoteHome: "/Users/razeenwasif",
      error: null,
      forwardedPorts: [{ localPort: 3000, remotePort: 3000, auto: true }],
    },
    catalog: {
      hosts: [{ alias: "mini", hostName: "100.97.247.34", user: "razeenwasif", port: 22 }],
      recents: [{ host: "mini", folder: "~/Github/Applications/Proton" }],
    },
    dirs: [
      { name: "Applications/Proton", path: "~/Github/Applications/Proton", isRepo: true, hasDevcontainer: true },
    ],
    draftHost: "mini",
    draftFolder: "~/Github/Applications/Proton",
    browsing: false,
    busy: false,
    error: null,
  });

  assert.match(html, /Disconnect \(Local\)/);
  assert.match(html, /Kagutsuchi-2\.local/);
  assert.match(html, /3000 auto/);
  assert.match(html, /Applications\/Proton/);
  assert.match(html, /devcontainer/);
  assert.match(html, /Open in New Window/);
});
