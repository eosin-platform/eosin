import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/');
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

test('closes dataset picker modal with Escape key', async ({ page }) => {
  const datasetsHeading = page.getByRole('heading', { name: 'Datasets' });
  const datasetDialog = page.getByRole('dialog', { name: 'Select dataset' });

  await page.locator('.dataset-picker-btn').click();
  await expect(datasetsHeading).toBeVisible();

  await datasetDialog.press('Escape');
  await expect(datasetsHeading).not.toBeVisible();
});

test('shows empty state when dataset search has no matches', async ({ page }) => {
  await page.locator('.dataset-picker-btn').click();
  await expect(page.getByRole('heading', { name: 'Datasets' })).toBeVisible();

  await page.getByRole('textbox', { name: 'Search datasets' }).fill('__playwright_no_dataset_matches__');
  await expect(page.getByText('No datasets found')).toBeVisible();

  await page.getByRole('button', { name: 'Close dataset picker' }).click();
});
