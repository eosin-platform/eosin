import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/');
});

test('loads main shell and connection status', async ({ page }) => {
  await expect(page).toHaveTitle(/Eosin/);
  await expect(page.locator('.sidebar')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Open settings' })).toBeVisible();
  await expect(page.locator('.connection-bar .status')).toBeVisible();
  await expect(page.locator('.section-title', { hasText: 'Slides' })).toBeVisible();
});
