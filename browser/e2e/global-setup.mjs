import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const workspaceRoot = path.resolve(__dirname, '..');

const namespace = process.env.E2E_K8S_NAMESPACE ?? 'eosin';
const deploymentName = process.env.E2E_K8S_BROWSER_DEPLOYMENT ?? 'eosin-browser';
const appPort = Number(process.env.E2E_APP_PORT ?? 4173);

const endpointKeys = ['META_ENDPOINT', 'IAM_ENDPOINT', 'FRUSTA_ENDPOINT'];

function runOrThrow(command, args) {
  const result = spawnSync(command, args, {
    cwd: workspaceRoot,
    encoding: 'utf8',
  });

  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} failed:\n${result.stderr || result.stdout}`);
  }

  return result.stdout.trim();
}

function parseJsonCommand(command, args) {
  const output = runOrThrow(command, args);
  try {
    return JSON.parse(output);
  } catch (error) {
    throw new Error(`Failed to parse JSON from ${command} ${args.join(' ')}:\n${output}`);
  }
}

function parseEndpoint(value) {
  try {
    const parsed = new URL(value);
    return {
      protocol: parsed.protocol,
      hostname: parsed.hostname,
      port: parsed.port ? Number(parsed.port) : null,
      path: parsed.pathname && parsed.pathname !== '/' ? parsed.pathname : '',
    };
  } catch {
    const parsed = new URL(`http://${value}`);
    return {
      protocol: parsed.protocol,
      hostname: parsed.hostname,
      port: parsed.port ? Number(parsed.port) : null,
      path: parsed.pathname && parsed.pathname !== '/' ? parsed.pathname : '',
    };
  }
}

function pickServicePort(service, preferredPort) {
  const ports = service?.spec?.ports ?? [];
  if (!Array.isArray(ports) || ports.length === 0) {
    throw new Error(`Service ${service?.metadata?.name ?? 'unknown'} has no ports`);
  }

  if (preferredPort) {
    const preferred = ports.find((portSpec) => Number(portSpec.port) === Number(preferredPort));
    if (preferred) {
      return Number(preferred.port);
    }
  }

  const httpPort = ports.find((portSpec) => Number(portSpec.port) === 80);
  if (httpPort) {
    return 80;
  }

  return Number(ports[0].port);
}

async function getAvailablePort(preferred, strict = false) {
  const tryListen = (port) =>
    new Promise((resolve) => {
      const server = net.createServer();
      server.once('error', () => resolve(false));
      server.once('listening', () => {
        server.close(() => resolve(true));
      });
      server.listen(port, '127.0.0.1');
    });

  if (await tryListen(preferred)) {
    return preferred;
  }

  if (strict) {
    throw new Error(`Required local port ${preferred} is unavailable`);
  }

  for (let port = preferred + 1; port < preferred + 200; port += 1) {
    if (await tryListen(port)) {
      return port;
    }
  }

  throw new Error(`Could not find a free local port near ${preferred}`);
}

async function waitForPortForward(child, label, timeoutMs = 30_000) {
  const start = Date.now();

  return new Promise((resolve, reject) => {
    let settled = false;

    const cleanup = () => {
      child.stdout?.off('data', onData);
      child.stderr?.off('data', onData);
      child.off('exit', onExit);
    };

    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      cleanup();
      fn(value);
    };

    const onData = (chunk) => {
      const text = String(chunk);
      if (text.includes('Forwarding from')) {
        finish(resolve);
      }
      if (text.toLowerCase().includes('error') || text.toLowerCase().includes('unable to listen')) {
        finish(reject, new Error(`[${label}] kubectl port-forward error: ${text}`));
      }
    };

    const onExit = (code) => {
      finish(reject, new Error(`[${label}] kubectl port-forward exited early with code ${code}`));
    };

    child.stdout?.on('data', onData);
    child.stderr?.on('data', onData);
    child.once('exit', onExit);

    const timer = setInterval(() => {
      if (Date.now() - start > timeoutMs) {
        clearInterval(timer);
        finish(reject, new Error(`[${label}] kubectl port-forward timed out after ${timeoutMs}ms`));
      }
      if (settled) {
        clearInterval(timer);
      }
    }, 100);
  });
}

async function waitForHttp(url, timeoutMs = 60_000) {
  const start = Date.now();

  while (Date.now() - start < timeoutMs) {
    try {
      const response = await fetch(url);
      if (response.ok || response.status < 500) {
        return;
      }
    } catch {
      // keep retrying
    }

    await sleep(500);
  }

  throw new Error(`Timed out waiting for ${url}`);
}

export default async function globalSetup() {
  const kubectl = process.env.KUBECTL_BIN ?? 'kubectl';

  runOrThrow(kubectl, ['version', '--client']);

  const deployment = parseJsonCommand(kubectl, ['get', 'deploy', '-n', namespace, deploymentName, '-o', 'json']);
  const container = deployment?.spec?.template?.spec?.containers?.find((item) => item.name === 'browser')
    ?? deployment?.spec?.template?.spec?.containers?.[0];

  if (!container) {
    throw new Error(`No container found in deployment ${deploymentName}`);
  }

  const envMap = new Map((container.env ?? []).map((entry) => [entry.name, entry.value]));

  const serviceRefs = endpointKeys.map((key) => {
    const value = envMap.get(key);
    if (!value) {
      throw new Error(`Deployment ${deploymentName} is missing ${key}`);
    }

    const parsed = parseEndpoint(value);
    return {
      key,
      original: value,
      serviceName: parsed.hostname,
      preferredPort: parsed.port,
      path: parsed.path,
    };
  });

  const preferredLocalPorts = {
    META_ENDPOINT: 3000,
    IAM_ENDPOINT: 39081,
    FRUSTA_ENDPOINT: 39082,
  };

  const childProcesses = [];
  const forwarded = [];

  for (const ref of serviceRefs) {
    const service = parseJsonCommand(kubectl, ['get', 'svc', '-n', namespace, ref.serviceName, '-o', 'json']);
    const remotePort = pickServicePort(service, ref.preferredPort);
    const localPort = await getAvailablePort(
      preferredLocalPorts[ref.key] ?? 39100,
      ref.key === 'META_ENDPOINT'
    );

    const child = spawn(
      kubectl,
      ['-n', namespace, 'port-forward', `svc/${ref.serviceName}`, `${localPort}:${remotePort}`],
      {
        cwd: workspaceRoot,
        stdio: ['ignore', 'pipe', 'pipe'],
      }
    );

    await waitForPortForward(child, `${ref.serviceName}:${localPort}->${remotePort}`);

    childProcesses.push(child);
    forwarded.push({
      key: ref.key,
      serviceName: ref.serviceName,
      localPort,
      remotePort,
      path: ref.path,
    });
  }

  const meta = forwarded.find((item) => item.key === 'META_ENDPOINT');
  const iam = forwarded.find((item) => item.key === 'IAM_ENDPOINT');
  const frusta = forwarded.find((item) => item.key === 'FRUSTA_ENDPOINT');

  if (!meta || !iam || !frusta) {
    throw new Error('Failed to initialize required service port-forwards');
  }

  const frustaPublicPath = parseEndpoint(envMap.get('PUBLIC_FRUSTA_ENDPOINT') ?? '/ws').path || '/ws';

  const injectedEnv = {
    ...process.env,
    NODE_ENV: process.env.NODE_ENV ?? 'test',
    META_ENDPOINT: `http://127.0.0.1:${meta.localPort}`,
    PUBLIC_META_ENDPOINT: `http://127.0.0.1:${meta.localPort}`,
    IAM_ENDPOINT: `http://127.0.0.1:${iam.localPort}`,
    PUBLIC_IAM_ENDPOINT: `http://127.0.0.1:${iam.localPort}`,
    FRUSTA_ENDPOINT: `http://127.0.0.1:${frusta.localPort}`,
    PUBLIC_FRUSTA_ENDPOINT: `ws://127.0.0.1:${frusta.localPort}${frustaPublicPath}`,
    PROTOCOL_HEADER: envMap.get('PROTOCOL_HEADER') ?? 'x-forwarded-proto',
    HOST_HEADER: envMap.get('HOST_HEADER') ?? 'x-forwarded-host',
  };

  process.stdout.write(
    `[e2e-init] Forwarded services: ${JSON.stringify(forwarded)}\n[e2e-init] Injected env: ${JSON.stringify({
      META_ENDPOINT: injectedEnv.META_ENDPOINT,
      PUBLIC_META_ENDPOINT: injectedEnv.PUBLIC_META_ENDPOINT,
      IAM_ENDPOINT: injectedEnv.IAM_ENDPOINT,
      PUBLIC_IAM_ENDPOINT: injectedEnv.PUBLIC_IAM_ENDPOINT,
      FRUSTA_ENDPOINT: injectedEnv.FRUSTA_ENDPOINT,
      PUBLIC_FRUSTA_ENDPOINT: injectedEnv.PUBLIC_FRUSTA_ENDPOINT,
    })}\n`
  );

  const appProcess = spawn('npm', ['run', 'dev', '--', '--host', '127.0.0.1', '--port', String(appPort)], {
    cwd: workspaceRoot,
    env: injectedEnv,
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  childProcesses.push(appProcess);

  appProcess.stdout?.on('data', (chunk) => {
    process.stdout.write(`[e2e-app] ${chunk}`);
  });
  appProcess.stderr?.on('data', (chunk) => {
    process.stderr.write(`[e2e-app] ${chunk}`);
  });

  await waitForHttp(`http://127.0.0.1:${appPort}`, 90_000);

  const runtimeDir = path.join(workspaceRoot, 'e2e');
  const runtimeFile = path.join(runtimeDir, '.runtime.json');
  fs.mkdirSync(runtimeDir, { recursive: true });
  fs.writeFileSync(
    runtimeFile,
    JSON.stringify(
      {
        namespace,
        deploymentName,
        appPort,
        injectedEnv: {
          META_ENDPOINT: injectedEnv.META_ENDPOINT,
          PUBLIC_META_ENDPOINT: injectedEnv.PUBLIC_META_ENDPOINT,
          IAM_ENDPOINT: injectedEnv.IAM_ENDPOINT,
          PUBLIC_IAM_ENDPOINT: injectedEnv.PUBLIC_IAM_ENDPOINT,
          FRUSTA_ENDPOINT: injectedEnv.FRUSTA_ENDPOINT,
          PUBLIC_FRUSTA_ENDPOINT: injectedEnv.PUBLIC_FRUSTA_ENDPOINT,
        },
        forwarded,
        pids: childProcesses.map((child) => child.pid),
      },
      null,
      2
    )
  );

  return async () => {
    for (const child of childProcesses.reverse()) {
      if (!child.killed) {
        child.kill('SIGTERM');
      }
    }

    await sleep(500);

    for (const child of childProcesses) {
      if (!child.killed) {
        child.kill('SIGKILL');
      }
    }

    if (fs.existsSync(runtimeFile)) {
      fs.rmSync(runtimeFile);
    }
  };
}
