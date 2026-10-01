import { test, expect } from '@playwright/test';
import { AxeBuilder } from '@axe-core/playwright';

test('search, theme, copy, headings and pagination', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  const errors: string[] = []; page.on('pageerror', error => errors.push(error.message));
  await page.goto('/docs/language/functions/');
  await page.selectOption('#theme', 'dark');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.reload(); await expect(page.locator('#theme')).toHaveValue('dark');
  await page.locator('.copy').first().click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain('func add');
  await page.keyboard.press('Control+k');
  await page.locator('#search-input').fill('strings');
  await expect(page.locator('.search-result').first()).toContainText('Strings');
  await page.keyboard.press('Enter'); await expect(page).toHaveURL(/standard-library\/strings/);
  await page.locator('.toc a[href="#split-and-join"]').click();
  await expect(page).toHaveURL(/#split-and-join/);
  await expect(page.locator('.toc a[href="#split-and-join"]')).toHaveAttribute('aria-current', 'location');
  await page.locator('.pagination a').last().click(); await expect(page).toHaveURL(/collections/);
  expect(errors).toEqual([]);
});

test('installation tabs and remembered navigation', async ({ page }) => {
  await page.goto('/docs/getting-started/installation/');
  await page.getByRole('tab', { name: 'Linux', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tabpanel')).toContainText('Unverified platform');
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tabpanel')).toContainText('Windows x86_64');
  const group = page.locator('.sidebar details').filter({ has: page.locator('summary', { hasText: 'Advanced' }) });
  await group.locator('summary').click();
  // The browser fires `toggle` after the attribute changes, and that is when
  // the preference is saved; reloading before it lands tests nothing.
  const key = `skuld-group-${await group.getAttribute('data-group')}`;
  await expect.poll(() => page.evaluate(name => localStorage.getItem(name), key)).toBe('closed');
  await page.reload(); await expect(group).not.toHaveAttribute('open');
});

test('mobile drawer, examples and honest playground', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/docs/'); await page.getByRole('button', { name: 'Open navigation' }).click();
  await expect(page.locator('#mobile-navigation')).toBeVisible();
  await page.locator('#mobile-navigation').getByRole('link', { name: 'Functions', exact: true }).click();
  await expect(page).toHaveURL(/language\/functions/);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBeTruthy();
  await page.goto('/examples/'); await page.getByRole('button', { name: 'Objects', exact: true }).click();
  await expect(page.locator('.example-card:visible')).toHaveCount(1);
  await page.locator('.example-card:visible [data-playground]').click();
  await expect(page.locator('#editor')).toHaveValue(/class Counter/);
  await page.locator('#run').click(); await expect(page.locator('#playground-output')).toContainText('execution is not available yet');
  await page.locator('#reset').click(); await expect(page.locator('#editor')).toHaveValue(/let name = "Skuld"/);
  const download = page.waitForEvent('download'); await page.locator('#download').click(); expect((await download).suggestedFilename()).toBe('main.skuld');
});

test('accessible reading, search and mobile surfaces in both themes', async ({ page }) => {
  for (const theme of ['dark', 'light']) {
    await page.goto('/docs/language/functions/'); await page.selectOption('#theme', theme);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    await page.keyboard.press('Control+k');
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    await page.keyboard.press('Escape');
  }
  await page.setViewportSize({ width: 390, height: 844 }); await page.locator('.mobile-toggle').click();
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test('404 and search empty state', async ({ page }) => {
  const response = await page.goto('/does-not-exist/'); expect(response?.status()).toBe(404);
  await page.getByRole('button', { name: 'Search documentation', exact: true }).click();
  await page.locator('#search-input').fill('zzzznotaword'); await expect(page.locator('#search-status')).toContainText('No results');
});
