import { expect, test, type Page } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.connection-bar .status')).toBeVisible();
});

async function openLoginModal(page: Page) {
  const loginHeading = page.getByRole('heading', { name: 'Login' });

  for (let cycle = 0; cycle < 2; cycle += 1) {
    const loginButton = page.locator('.auth-btn.login-btn');
    await expect(loginButton).toBeVisible();

    if (await loginHeading.isVisible().catch(() => false)) {
      return;
    }

    for (let attempt = 0; attempt < 3; attempt += 1) {
      await loginButton.focus();
      await loginButton.press('Enter');
      if (await loginHeading.isVisible().catch(() => false)) {
        return;
      }

      await loginButton.click();
      if (await loginHeading.isVisible().catch(() => false)) {
        return;
      }

      await page.waitForTimeout(150);
    }

    if (cycle === 0) {
      await page.reload();
      await expect(page.locator('.connection-bar .status')).toBeVisible();
    }
  }

  await expect(loginHeading).toBeVisible();
}

function getLoginCredentials() {
  return {
    username: process.env.TEST_USERNAME,
    password: process.env.TEST_PASSWORD,
  };
}

test('shows validation error when submitting empty login form', async ({ page }) => {
  await openLoginModal(page);

  await page.locator('.login-modal button[type="submit"]').click();
  await expect(page.getByText('Please enter both username and password')).toBeVisible();

  await page.getByRole('button', { name: 'Close login' }).click();
  await expect(page.getByRole('heading', { name: 'Login' })).not.toBeVisible();
});

test('shows error for invalid username/password', async ({ page }) => {
  await openLoginModal(page);

  await page.locator('#username').fill('__playwright_invalid_username__');
  await page.locator('#password').fill('__playwright_invalid_password__');
  await page.locator('.login-modal button[type="submit"]').click();

  await expect(page.getByText('Invalid username or password')).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Login' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Logout' })).not.toBeVisible();
});

test('logs in successfully with TEST_USERNAME/TEST_PASSWORD and can logout', async ({ page }) => {
  const { username, password } = getLoginCredentials();
  test.skip(!username || !password, 'TEST_USERNAME and TEST_PASSWORD must be set from .env');

  await openLoginModal(page);

  await page.locator('#username').fill(username!);
  await page.locator('#password').fill(password!);
  await page.locator('.login-modal button[type="submit"]').click();

  await expect(page.getByRole('heading', { name: 'Login' })).not.toBeVisible();
  await expect(page.getByRole('button', { name: 'Logout' })).toBeVisible();
  await expect(page.locator('.auth-btn.login-btn')).not.toBeVisible();

  await expect
    .poll(async () => {
      const cookies = await page.context().cookies();
      return cookies.some((cookie) => cookie.name === 'eosin_refresh_token');
    })
    .toBe(true);

  await page.getByRole('button', { name: 'Logout' }).click();

  await expect(page.locator('.auth-btn.login-btn')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Logout' })).not.toBeVisible();

  await expect
    .poll(async () => {
      const cookies = await page.context().cookies();
      return cookies.some((cookie) => cookie.name === 'eosin_refresh_token');
    })
    .toBe(false);
});
