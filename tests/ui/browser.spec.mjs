import {test, expect} from '@playwright/test';

test('production UI passes the native-transport regression harness', async ({page}, testInfo) => {
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto('/tests/ui/index.html');
  try {
    await expect(page.locator('#status')).toHaveText(/^\d+ passed · \d+ failed$/, {timeout: 100000});
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
