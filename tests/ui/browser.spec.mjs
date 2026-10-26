import {test, expect} from '@playwright/test';

test('production UI passes the native-transport regression harness', async ({page}, testInfo) => {
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto('/tests/ui/index.html');
  try {
    await expect(page.locator('#status')).toHaveText(/^\d+ passed · \d+ failed$/, {timeout: 220000});
    const results = await page.locator('#results li').allTextContents();
    await testInfo.attach('regression-results', {body: results.join('\n'), contentType: 'text/plain'});
    expect(results.length).toBeGreaterThanOrEqual(58);
    expect(results.filter(result => !result.startsWith('PASS — '))).toEqual([]);
    await expect(page.locator('#status')).toHaveText(/^\d+ passed · 0 failed$/);
    expect(errors).toEqual([]);
  } finally {
    await testInfo.attach('runtime-errors', {body: errors.join('\n'), contentType: 'text/plain'});
  }
});

test('standalone preview loads its adapter and switches modes', async ({page}) => {
  const errors = [];
  const fixtures = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    if (message.type() === 'error') errors.push(message.text());
  });
  page.on('request', request => {
    if (new URL(request.url()).pathname === '/preview-bridge.js') fixtures.push(request.url());
  });
  await page.goto('/');
  await expect(page.locator('.workspace.on [data-editor] .ln').first()).toBeVisible();
  expect(fixtures).toHaveLength(1);
  await page.getByRole('tab', {name: 'Data', exact: true}).click();
  await expect(page.getByRole('tab', {name: 'Data', exact: true})).toHaveAttribute('aria-selected', 'true');
  await page.getByRole('tab', {name: 'Codex', exact: true}).click();
  await expect(page.locator('.workspace.on [data-editor] .ln').first()).toBeVisible();
  expect(errors).toEqual([]);
});

test('Game mode\'s renderer draws its art, hazed by its plane, over the ground', async ({page}) => {
  await page.goto('/tests/ui/game-render.html');
  await page.waitForFunction(() => window.result !== undefined);
  const result = await page.evaluate(() => window.result);
  expect(result.gl).toBe(true);
  const near = (got, want) => got.slice(0, 3).every((v, i) => Math.abs(v - want[i]) <= 3);
  expect(near(result.left, [255, 0, 0])).toBe(true);
  expect(near(result.right, [128, 0, 128])).toBe(true);
  expect(near(result.ground, [0, 255, 0])).toBe(true);
  // A skeleton's part where its bone carries it, between two sprites drawn
  // by the other program, and flipped about its feet.
  expect(result.skel.map((p, i) => near(p, [[0, 0, 255], [0, 255, 0], [255, 0, 0], [255, 0, 0]][i]))).toEqual([true, true, true, true]);
  expect(result.flipped.map((p, i) => near(p, [[0, 0, 255], [0, 255, 0]][i]))).toEqual([true, true]);
  // Light: red where the light falls, the dim ambient beyond its reach;
  // unlit art as painted.
  expect([near(result.lit[0], [255, 26, 26]), near(result.lit[1], [26, 26, 26]), near(result.unlit, [255, 255, 255])]).toEqual([true, true, true]);
  // A normal map: lit from the side it faces, not from behind.
  expect(result.normal[0][0]).toBeGreaterThan(150);
  expect(near(result.normal[1], [26, 26, 26])).toBe(true);
  // Bloom spills only when the picture is finished, and not at low quality.
  expect(result.bloom[0][0]).toBeGreaterThan(20);
  expect([result.bloom[1][0], result.bloom[2][0]]).toEqual([0, 0]);
  // The grade turns red to cyan; the vignette darkens the corner alone.
  expect(near(result.graded, [0, 255, 255])).toBe(true);
  expect(near(result.vignette[0], [255, 255, 255])).toBe(true);
  expect(result.vignette[1][0]).toBeLessThan(160);
});

test('Game mode\'s runtime compiles and runs under the app\'s own CSP, which still refuses eval', async ({page}) => {
  const response = await page.goto('/tests/ui/game-runtime.html?csp=1');
  expect(response.headers()['content-security-policy']).toContain("'wasm-unsafe-eval'");
  await page.waitForFunction(() => window.result !== undefined);
  const result = await page.evaluate(() => window.result);
  expect(result.error).toBeUndefined();
  expect(result.eval).toBe('refused');
  expect(result.hash).toBe(result.want);
  // The one refusal is the control's: eval, never the WebAssembly.
  expect(result.violations.filter((v) => !/: eval$/.test(v))).toEqual([]);
});

test('Canvas\'s engine runs under the app\'s CSP: its composite uploaded from its memory, which grows to a gigabyte and no further', async ({page}) => {
  await page.goto('/tests/ui/raster-probe.html?csp=1');
  await page.waitForFunction(() => window.result !== undefined);
  const result = await page.evaluate(() => window.result);
  expect(result.error).toBeUndefined();
  // A 256 × 256 canvas is sixteen tiles, all laid together when it opens.
  expect(result.rects).toBe(16);
  expect(result.gl).toBe(true);
  expect(result.inside).toEqual([255, 0, 0, 255]);
  expect(result.outside).toEqual([255, 255, 255, 255]);
  expect(result.grew).toBe(true);
  expect(result.beyond).toBe('RangeError');
});
