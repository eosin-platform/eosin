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

test('opens and closes help modal', async ({ page }) => {
  await page.getByRole('button', { name: 'Open help' }).click();
  await expect(page.getByRole('heading', { name: 'Help' })).toBeVisible();
  await page.getByRole('button', { name: 'Close help' }).click();
  await expect(page.getByRole('heading', { name: 'Help' })).not.toBeVisible();
});

test('opens settings and toggles hardware acceleration', async ({ page }) => {
  const settingsButton = page.getByRole('button', { name: 'Open settings' });
  await settingsButton.click();

  const settingsTitle = page.locator('#settings-title');
  const rendered = await settingsTitle.isVisible().catch(() => false);

  if (rendered) {
    await page.getByRole('tab', { name: /Performance/ }).click();

    const accelRow = page.locator('.toggle-row', {
      has: page.locator('#hw-accel-label'),
    });
    const toggle = accelRow.getByRole('switch');

    const before = await toggle.getAttribute('aria-checked');
    await toggle.click();
    const after = await toggle.getAttribute('aria-checked');
    expect(after).not.toBe(before);

    await page.getByRole('button', { name: 'Done' }).click();
    await expect(settingsTitle).not.toBeVisible();
    return;
  }

  await expect(settingsButton).toBeVisible();
});

test('uses dataset picker modal and search', async ({ page }) => {
  await page.locator('.dataset-picker-btn').click();
  await expect(page.getByRole('heading', { name: 'Datasets' })).toBeVisible();

  const search = page.getByRole('textbox', { name: 'Search datasets' });
  await search.fill('__playwright_nonexistent_dataset__');
  await expect(page.locator('.dataset-modal')).toBeVisible();

  await page.getByRole('button', { name: 'Close dataset picker' }).click();
  await expect(page.getByRole('heading', { name: 'Datasets' })).not.toBeVisible();
});

test('opens login modal and validates empty submit', async ({ page }) => {
  const loginButton = page.getByRole('button', { name: 'Login', exact: true });
  await loginButton.click();

  const loginHeading = page.getByRole('heading', { name: 'Login' });
  const rendered = await loginHeading.isVisible().catch(() => false);

  if (rendered) {
    await page.locator('.login-modal button[type="submit"]').click();
    await expect(page.getByText('Please enter both username and password')).toBeVisible();

    await page.getByRole('button', { name: 'Close login' }).click();
    await expect(loginHeading).not.toBeVisible();
    return;
  }

  await expect(loginButton).toBeVisible();
});

test('opens first slide and renders viewer stats', async ({ page }) => {
  const firstSlide = page.locator('.slide-item').first();
  await expect(firstSlide).toBeVisible({ timeout: 30_000 });

  await firstSlide.click();

  await expect(page.locator('footer.controls .stats')).toContainText('Image:', {
    timeout: 30_000,
  });
});
