// The game's runtime compiled as Game mode compiles it — the bytes fetched,
// then `WebAssembly.compile` — and the fixture's replay run to its end. Under
// the app's CSP this is what 'wasm-unsafe-eval' has to allow; `eval` is tried
// too, as the control that the policy is really on: it must stay refused.
const result = { violations: [] };
document.addEventListener('securitypolicyviolation', (e) => result.violations.push(`${e.violatedDirective}: ${e.blockedURI}`));
try {
  // eslint-disable-next-line no-eval
  result.eval = (0, eval)('"allowed"');
} catch {
  result.eval = 'refused';
}
try {
  const bytes = await (await fetch('/game-runtime.wasm')).arrayBuffer();
  const module = await WebAssembly.compile(bytes);
  const pg = (await WebAssembly.instantiate(module, {})).exports;
  const world = new Uint8Array(await (await fetch('/tests/ui/fixtures/game-world.json')).arrayBuffer());
  const fixture = await (await fetch('/tests/ui/fixtures/game-replay.json')).json();
  const at = pg.pg_alloc(world.length);
  new Uint8Array(pg.memory.buffer, at, world.length).set(world);
  if (pg.pg_load(at, world.length) !== 0) throw new Error('the world did not load');
  pg.pg_free(at, world.length);
  const [room, x, y, abilities] = fixture.start;
  if (pg.pg_start(room, x, y, abilities) !== 0) throw new Error('the run did not start');
  for (const [held, n] of fixture.replay) for (let i = 0; i < n; i++) pg.pg_step(held);
  result.hash = (BigInt(pg.pg_hash_hi() >>> 0) << 32n | BigInt(pg.pg_hash_lo() >>> 0)).toString(16).padStart(16, '0');
  result.want = fixture.hash;
} catch (error) {
  result.error = String(error);
}
window.result = result;
document.getElementById('out').textContent = JSON.stringify(result);
