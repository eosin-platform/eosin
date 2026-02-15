import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/');
});

test('opens first slide and renders viewer stats', async ({ page }) => {
  const firstSlide = page.locator('.slide-item').first();
  await expect(firstSlide).toBeVisible({ timeout: 30_000 });

  await firstSlide.click();

  await expect(page.locator('footer.controls .stats')).toContainText('Image:', {
    timeout: 30_000,
  });
});
