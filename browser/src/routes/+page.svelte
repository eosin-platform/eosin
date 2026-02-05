<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { createFrustaClient, type ConnectionState, type TileData } from '$lib/frusta';

  let connectionState = $state<ConnectionState>('disconnected');
  let tilesReceived = $state(0);
  let lastError = $state<string | null>(null);
  let wsUrl = $state('ws://localhost:8080/ws');

  let client: ReturnType<typeof createFrustaClient> | null = null;

  function connect() {
    if (client) {
      client.disconnect();
    }

    lastError = null;
    client = createFrustaClient({
      url: wsUrl,
      reconnectDelay: 1000,
      maxReconnectAttempts: 5,
      onStateChange: (state) => {
        connectionState = state;
      },
      onTile: (tile: TileData) => {
        tilesReceived++;
        console.log(`Received tile: slot=${tile.slot}, x=${tile.meta.x}, y=${tile.meta.y}, level=${tile.meta.level}, size=${tile.data.length}`);
      },
      onOpenResponse: (response) => {
        console.log(`Slide opened: slot=${response.slot}`);
      },
      onError: (error) => {
        lastError = error instanceof Error ? error.message : 'Connection error';
        console.error('WebSocket error:', error);
      },
    });

    client.connect();
  }

  function disconnect() {
    client?.disconnect();
    client = null;
  }

  onDestroy(() => {
    client?.disconnect();
  });
</script>

<main>
  <h1>Frusta Viewer</h1>

  <section class="connection">
    <label>
      WebSocket URL:
      <input type="text" bind:value={wsUrl} placeholder="ws://localhost:8080/ws" />
    </label>

    <div class="controls">
      {#if connectionState === 'disconnected' || connectionState === 'error'}
        <button onclick={connect}>Connect</button>
      {:else}
        <button onclick={disconnect}>Disconnect</button>
      {/if}
    </div>

    <div class="status">
      <span class="status-indicator" class:connected={connectionState === 'connected'} class:connecting={connectionState === 'connecting'} class:error={connectionState === 'error'}></span>
      <span class="status-text">{connectionState}</span>
    </div>

    {#if lastError}
      <p class="error">{lastError}</p>
    {/if}

    <p>Tiles received: {tilesReceived}</p>
  </section>
</main>

<style>
  main {
    max-width: 800px;
    margin: 0 auto;
    padding: 2rem;
  }

  h1 {
    margin-bottom: 2rem;
  }

  .connection {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  input {
    padding: 0.5rem;
    font-size: 1rem;
    border: 1px solid #ccc;
    border-radius: 4px;
  }

  .controls {
    display: flex;
    gap: 0.5rem;
  }

  button {
    padding: 0.5rem 1rem;
    font-size: 1rem;
    cursor: pointer;
    border: none;
    border-radius: 4px;
    background-color: #007bff;
    color: white;
  }

  button:hover {
    background-color: #0056b3;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .status-indicator {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background-color: #6c757d;
  }

  .status-indicator.connected {
    background-color: #28a745;
  }

  .status-indicator.connecting {
    background-color: #ffc107;
  }

  .status-indicator.error {
    background-color: #dc3545;
  }

  .error {
    color: #dc3545;
  }
</style>
