# Eosin Browser (WSI Viewer)

This is Eosin’s web UI for interactive whole-slide imaging (WSI) viewing and annotation.

Demo: https://app.eosin.ai/

## Prerequisites

- Node.js + npm

## Install

```bash
npm install
```

## Development

```bash
npm run dev
```

This app talks to Eosin backend services.
When running outside the cluster, you typically point it at port-forwards.

### Required environment variables

- `META_ENDPOINT`: server-side HTTP endpoint for the `meta` service (SSR/API routes).
- `IAM_ENDPOINT`: server-side HTTP endpoint for the `iam` service (SSR auth API routes).
- `PUBLIC_META_ENDPOINT`: browser-reachable HTTP endpoint for `meta` (client-side annotation calls).
- `PUBLIC_FRUSTA_ENDPOINT`: browser-reachable WebSocket URL for `frusta` (tile streaming).

Example `.env.local`:

```bash
META_ENDPOINT=http://127.0.0.1:8080
IAM_ENDPOINT=http://127.0.0.1:8081
PUBLIC_META_ENDPOINT=http://127.0.0.1:8080
PUBLIC_FRUSTA_ENDPOINT=ws://127.0.0.1:8082
```

## Build + preview

```bash
npm run build
npm run preview
```

## Quality

```bash
npm run check
npm run lint
npm run format
```

## End-to-End tests

This package uses Playwright for E2E coverage of major UI flows.

### What test init does

On Playwright global setup (`e2e/global-setup.mjs`), the test harness:

1. Runs `kubectl` against the `eosin` namespace.
2. Reads env injection from the `eosin-browser` deployment (same source of truth as the cluster app).
3. Resolves service ports and derives local values for `META_ENDPOINT`, `IAM_ENDPOINT`, `FRUSTA_ENDPOINT` and the corresponding `PUBLIC_*` vars.
4. Starts `kubectl port-forward` processes to those services.
5. Starts the local dev server with env vars injected to use those forwarded localhost endpoints.

### Run tests

```bash
npm run e2e
```

Playwright automatically loads environment variables from these files (first file has highest precedence):

- `.env.test.local`
- `.env.test`
- `.env.local`
- `.env`

Optional:

```bash
npm run e2e:headed
npm run e2e:debug
```

### Prerequisites

- `kubectl` installed and authenticated.
- Access to namespace `eosin`.
- An `eosin-browser` deployment present in that namespace.
