browser front end for the WSI viewer

## End-to-End tests

This repo uses Playwright for E2E coverage of major UI flows.

### What test init does

On Playwright global setup (`e2e/global-setup.mjs`), the test harness:

1. Runs `kubectl` against the `eosin` namespace.
2. Reads env injection from the `eosin-browser` deployment (same source of truth as the cluster app).
3. Resolves service ports for `META_ENDPOINT`, `IAM_ENDPOINT`, and `FRUSTA_ENDPOINT`.
4. Starts `kubectl port-forward` processes to those services.
5. Starts the local Svelte dev server with env vars injected to use those forwarded localhost endpoints.

### Run tests

```bash
npm run e2e
```

Optional:

```bash
npm run e2e:headed
npm run e2e:debug
```

### Prerequisites

- `kubectl` installed and authenticated.
- Access to namespace `eosin`.
- `eosin-browser` deployment present in that namespace.
