import test from "node:test";
import assert from "node:assert/strict";

import { statusSummary, renderPortsHtml, renderContainerHtml } from "../../ui/container.js";

const TEMPLATES = [
  { id: "rust", label: "Rust", description: "Rust toolchain" },
  { id: "python", label: "Python", description: "Python toolchain" },
];

function makeState(overrides = {}) {
  return {
    phase: "detected",
    name: "Demo Container",
    configPath: ".devcontainer/devcontainer.json",
    configs: [
      { path: ".devcontainer/devcontainer.json", name: "Demo Container", source: "rust:1" },
    ],
    image: "rust:1",
    workspaceFolder: "/workspaces/demo",
    remoteUser: "vscode",
    runtime: "docker",
    containerId: null,
    stepNote: null,
    ports: [],
    error: null,
    logs: [],
    suggestedTemplate: "rust",
    templates: TEMPLATES,
    ...overrides,
  };
}

test("statusSummary covers each container lifecycle phase", () => {
  assert.deepEqual(statusSummary(makeState({ phase: "absent" })), {
    label: "No config",
    tone: "",
    dot: false,
    statusText: "",
  });
  assert.equal(statusSummary(makeState({ phase: "detected", name: "Py" })).statusText, "Container: Py");
  assert.equal(statusSummary(makeState({ phase: "building" })).tone, "busy");
  assert.equal(statusSummary(makeState({ phase: "starting" })).dot, true);
  assert.equal(statusSummary(makeState({ phase: "running", name: "RustDev" })).tone, "ok");
  assert.equal(statusSummary(makeState({ phase: "running", name: "", containerId: "abc123" })).statusText, "Container: abc123");
  assert.equal(statusSummary(makeState({ phase: "stopping" })).tone, "warn");
  assert.equal(statusSummary(makeState({ phase: "error" })).tone, "err");
});

test("renderPortsHtml formats identical and remapped ports safely", () => {
  assert.equal(renderPortsHtml([]), "");
  const html = renderPortsHtml([
    { hostPort: 3000, containerPort: 3000, label: "Web <UI>", protocol: "tcp", url: "http://127.0.0.1:3000" },
    { hostPort: 8081, containerPort: 8080, label: null, protocol: "tcp", url: "http://127.0.0.1:8081" },
  ]);
  assert.match(html, /3000\/tcp/);
  assert.match(html, /8081 → 8080\/tcp/);
  assert.match(html, /Web &lt;UI&gt;/);
  assert.doesNotMatch(html, /<UI>/);
});

test("renderContainerHtml renders template picker in absent phase", () => {
  const html = renderContainerHtml(makeState({ phase: "absent", suggestedTemplate: "python" }));
  assert.match(html, /Create \.devcontainer\/devcontainer\.json/);
  assert.match(html, /data-dc-create="python"/);
  assert.match(html, /Suggested/);

  const explicit = renderContainerHtml(makeState({ phase: "absent", suggestedTemplate: "python" }), "rust");
  assert.match(explicit, /data-dc-create="rust"/);
});

test("renderContainerHtml renders detected, building, running, and error states", () => {
  const detected = renderContainerHtml(makeState({ phase: "detected" }));
  assert.match(detected, /Start &amp; Attach/);
  assert.match(detected, /data-dc-act="rebuild"/);

  const building = renderContainerHtml(makeState({ phase: "building", stepNote: "Pulling image…" }));
  assert.match(building, /Pulling image…/);
  assert.match(building, /disabled/);

  const running = renderContainerHtml(
    makeState({
      phase: "running",
      containerId: "deadbeef1234",
      ports: [{ hostPort: 5173, containerPort: 5173, label: "Vite", protocol: "tcp", url: "http://127.0.0.1:5173" }],
      logs: ["line 1", "line <2>"],
    }),
  );
  assert.match(running, /data-dc-act="terminal"/);
  assert.match(running, /data-dc-act="detach"/);
  assert.match(running, /data-dc-act="stop"/);
  assert.match(running, /deadbeef1234/);
  assert.match(running, /5173\/tcp/);
  assert.match(running, /line &lt;2&gt;/);

  const errored = renderContainerHtml(makeState({ phase: "error", error: "Docker daemon stopped" }));
  assert.match(errored, /Docker daemon stopped/);
  assert.match(errored, /Runtime Settings/);
});
