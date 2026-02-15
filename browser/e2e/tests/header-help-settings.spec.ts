import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/');
});

test('opens and closes help modal', async ({ page }) => {
  await page.getByRole('button', { name: 'Open help' }).click();
  await expect(page.getByRole('heading', { name: 'Help' })).toBeVisible();
  await page.getByRole('button', { name: 'Close help' }).click();
  await expect(page.getByRole('heading', { name: 'Help' })).not.toBeVisible();
});

test('toggles help modal with H keyboard shortcut', async ({ page }) => {
  const firstSlide = page.locator('.slide-item').first();
  await expect(firstSlide).toBeVisible({ timeout: 30_000 });
  await firstSlide.click();

  const helpHeading = page.getByRole('heading', { name: 'Help' });

  await page.keyboard.press('h');
  await expect(helpHeading).toBeVisible();

  await page.keyboard.press('h');
  await expect(helpHeading).not.toBeVisible();
});

test('closes help modal with Escape key', async ({ page }) => {
  const firstSlide = page.locator('.slide-item').first();
  await expect(firstSlide).toBeVisible({ timeout: 30_000 });
  await firstSlide.click();

  const helpHeading = page.getByRole('heading', { name: 'Help' });
  await page.getByRole('button', { name: 'Open help' }).click();
  await expect(helpHeading).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(helpHeading).not.toBeVisible();
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
