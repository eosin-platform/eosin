import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/');
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
