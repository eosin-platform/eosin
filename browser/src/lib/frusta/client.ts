/**
 * Frusta WebSocket client for connecting to the tile streaming service.
 */

import {
  type ImageDesc,
  type Viewport,
  type TileData,
  type OpenResponse,
  type ProgressEvent,
  type SlideCreatedEvent,
  buildOpenMessage,
  buildUpdateMessage,
  buildCloseMessage,
  buildClearCacheMessage,
  buildRequestTileMessage,
  parseOpenResponse,
  parseTileData,
  parseProgressEvent,
  parseSlideCreated,
  isOpenResponse,
  isProgressEvent,
  isRateLimited,
  isSlideCreated,
} from './protocol';

export type ConnectionState = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface FrustaClientOptions {
  /** WebSocket URL (e.g., "ws://localhost:8080/ws") */
  url: string;
  /** Reconnect delay in milliseconds (default: 1000) */
  reconnectDelay?: number;
  /** Maximum reconnect attempts (default: 5, 0 = infinite) */
  maxReconnectAttempts?: number;
  /** Connection timeout in milliseconds (default: 10000) */
  connectTimeout?: number;
  /** Called when connection state changes */
  onStateChange?: (state: ConnectionState) => void;
  /** Called when a tile is received */
  onTile?: (tile: TileData) => void;
  /** Called when an open response is received */
  onOpenResponse?: (response: OpenResponse) => void;
  /** Called when a progress event is received */
  onProgress?: (event: ProgressEvent) => void;
  /** Called when a slide created event is received */
  onSlideCreated?: (event: SlideCreatedEvent) => void;
  /** Called when the server signals the client is being rate limited */
  onRateLimited?: () => void;
  /** Called on error */
  onError?: (error: Event | Error) => void;
  /**
   * Called when a slide's slot is reassigned after reconnection.
   * The consumer must update any references to the old slot.
   */
  onSlotReassigned?: (id: Uint8Array, oldSlot: number, newSlot: number) => void;
}

/** Internal record of an open slide for reconnection. */
interface TrackedSlide {
  dpi: number;
  image: ImageDesc;
  slot: number;
}

export class FrustaClient {
  private ws: WebSocket | null = null;
  private options: Required<FrustaClientOptions>;
  private reconnectAttempts = 0;
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private _state: ConnectionState = 'disconnected';
  private intentionalClose = false;
  private _rateLimited = false;
  private connectTimeoutHandle: ReturnType<typeof setTimeout> | null = null;
  private rateLimitCooldownTimeout: ReturnType<typeof setTimeout> | null = null;
  /**
   * Slides currently considered open by the client.
   * Key is the hex-encoded UUID for easy comparison.
   */
  private openSlides = new Map<string, TrackedSlide>();
  /** True while we are re-opening slides after a reconnect. */
  private _reconnecting = false;

  constructor(options: FrustaClientOptions) {
    this.options = {
      reconnectDelay: 1000,
      maxReconnectAttempts: 5,
      connectTimeout: 10000,
      onStateChange: () => {},
      onTile: () => {},
      onOpenResponse: () => {},
      onProgress: () => {},
      onSlideCreated: () => {},
      onRateLimited: () => {},
      onError: () => {},
      onSlotReassigned: () => {},
      ...options,
    };
  }

  /** Current connection state */
  get state(): ConnectionState {
    return this._state;
  }

  private setState(state: ConnectionState) {
    if (this._state !== state) {
      this._state = state;
      this.options.onStateChange(state);
    }
  }

  /** Whether the client is currently rate-limited (5-second cooldown after server notification) */
  get rateLimited(): boolean {
    return this._rateLimited;
  }

  /** Whether the client is currently re-opening slides after a reconnection */
  get reconnecting(): boolean {
    return this._reconnecting;
  }

  /** Convert a UUID Uint8Array to a hex string key for the openSlides map. */
  private static idToKey(id: Uint8Array): string {
    return Array.from(id)
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('');
  }

  /** Enter rate-limited state for 5 seconds */
  private enterRateLimitCooldown(): void {
    this._rateLimited = true;
    if (this.rateLimitCooldownTimeout) {
      clearTimeout(this.rateLimitCooldownTimeout);
    }
    this.rateLimitCooldownTimeout = setTimeout(() => {
      this._rateLimited = false;
      this.rateLimitCooldownTimeout = null;
    }, 5000);
  }

  /** Connect to the frusta WebSocket server */
  connect(): void {
    if (this.ws?.readyState === WebSocket.OPEN || this.ws?.readyState === WebSocket.CONNECTING) {
      return;
    }

    this.intentionalClose = false;
    this.setState('connecting');

    try {
      this.ws = new WebSocket(this.options.url);
      this.ws.binaryType = 'arraybuffer';

      // Set up connection timeout
      if (this.options.connectTimeout > 0) {
        this.connectTimeoutHandle = setTimeout(() => {
          this.connectTimeoutHandle = null;
          if (this.ws?.readyState === WebSocket.CONNECTING) {
            this.options.onError(new Error('Connection timed out'));
            this.ws.close();
          }
        }, this.options.connectTimeout);
      }

      this.ws.onopen = () => {
        if (this.connectTimeoutHandle) {
          clearTimeout(this.connectTimeoutHandle);
          this.connectTimeoutHandle = null;
        }
        this.reconnectAttempts = 0;
        this.setState('connected');
        this.reopenTrackedSlides();
      };

      this.ws.onclose = (event) => {
        this.ws = null;
        if (this.intentionalClose) {
          this.setState('disconnected');
        } else {
          this.handleReconnect();
        }
      };

      this.ws.onerror = (event) => {
        this.options.onError(event);
        this.setState('error');
      };

      this.ws.onmessage = (event) => {
        this.handleMessage(event.data);
      };
    } catch (error) {
      this.options.onError(error as Error);
      this.setState('error');
      this.handleReconnect();
    }
  }

  /** Disconnect from the server */
  disconnect(): void {
    this.intentionalClose = true;
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }
    if (this.connectTimeoutHandle) {
      clearTimeout(this.connectTimeoutHandle);
      this.connectTimeoutHandle = null;
    }
    if (this.rateLimitCooldownTimeout) {
      clearTimeout(this.rateLimitCooldownTimeout);
      this.rateLimitCooldownTimeout = null;
    }
    this._rateLimited = false;
    this.openSlides.clear();
    this._reconnecting = false;
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    this.setState('disconnected');
  }

  /**
   * Re-open all tracked slides after a reconnection.
   * Called automatically when the WebSocket connects/reconnects.
   */
  private reopenTrackedSlides(): void {
    if (this.openSlides.size === 0) return;
    this._reconnecting = true;
    for (const tracked of this.openSlides.values()) {
      this.send(buildOpenMessage(tracked.dpi, tracked.image));
    }
    // _reconnecting is cleared once all OpenResponses arrive,
    // but as a safety net clear it after a short delay.
    setTimeout(() => {
      this._reconnecting = false;
    }, 5000);
  }

  private handleReconnect(): void {
    if (this.intentionalClose) return;

    const maxAttempts = this.options.maxReconnectAttempts;
    if (maxAttempts > 0 && this.reconnectAttempts >= maxAttempts) {
      this.setState('error');
      return;
    }

    this.reconnectAttempts++;
    const delay = this.options.reconnectDelay * Math.min(this.reconnectAttempts, 10);

    this.reconnectTimeout = setTimeout(() => {
      this.connect();
    }, delay);
  }

  private handleMessage(data: ArrayBuffer): void {
    // Check if this is a RateLimited notification
    if (isRateLimited(data)) {
      this.enterRateLimitCooldown();
      this.options.onRateLimited();
      return;
    }

    // Check if this is an Open response (message type byte is first)
    if (isOpenResponse(data)) {
      const response = parseOpenResponse(data);
      if (response) {
        const key = FrustaClient.idToKey(response.id);
        const tracked = this.openSlides.get(key);
        if (tracked) {
          const oldSlot = tracked.slot;
          tracked.slot = response.slot;
          // Only fire onSlotReassigned when a previously-assigned slot changes
          // (oldSlot === -1 means this is the first assignment, not a reassignment)
          if (oldSlot !== -1 && oldSlot !== response.slot) {
            this.options.onSlotReassigned(response.id, oldSlot, response.slot);
          }
        }
        this.options.onOpenResponse(response);
        return;
      }
    }

    // Check if this is a Progress event
    if (isProgressEvent(data)) {
      const event = parseProgressEvent(data);
      console.log('Received progress event', { event });
      if (event) {
        this.options.onProgress(event);
        return;
      }
    }

    // Check if this is a SlideCreated event
    if (isSlideCreated(data)) {
      const event = parseSlideCreated(data);
      console.log('Received slide created event', { event });
      if (event) {
        this.options.onSlideCreated(event);
        return;
      }
    }

    // Otherwise, treat as tile data
    const tile = parseTileData(data);
    if (tile) {
      this.options.onTile(tile);
    }
  }

  /** Send a raw binary message */
  private send(data: ArrayBuffer): boolean {
    if (this.ws?.readyState !== WebSocket.OPEN) {
      return false;
    }
    this.ws.send(data);
    return true;
  }

  /**
   * Open a slide for viewing.
   * @param dpi Device DPI for tile scaling
   * @param image Image descriptor with UUID and dimensions
   */
  openSlide(dpi: number, image: ImageDesc): boolean {
    const sent = this.send(buildOpenMessage(dpi, image));
    // Track regardless of send success — if disconnected, will be re-opened on reconnect
    const key = FrustaClient.idToKey(image.id);
    if (!this.openSlides.has(key)) {
      this.openSlides.set(key, { dpi, image, slot: -1 });
    }
    return sent;
  }

  /**
   * Update the viewport for a slide.
   * @param slot The slot number returned from openSlide
   * @param viewport Current viewport state
   */
  updateViewport(slot: number, viewport: Viewport): boolean {
    return this.send(buildUpdateMessage(slot, viewport));
  }

  /**
   * Close a slide.
   * @param id The UUID of the slide to close
   */
  closeSlide(id: Uint8Array): boolean {
    this.openSlides.delete(FrustaClient.idToKey(id));
    return this.send(buildCloseMessage(id));
  }

  /**
   * Clear the tile cache for a slot.
   * @param slot The slot number to clear
   */
  clearCache(slot: number): boolean {
    return this.send(buildClearCacheMessage(slot));
  }

  /**
   * Request a specific tile from the server.
   * Used for retrying tiles that weren't received in time.
   * @param slot The slot number
   * @param x Tile X coordinate
   * @param y Tile Y coordinate
   * @param level Mip level
   */
  requestTile(slot: number, x: number, y: number, level: number): boolean {
    if (this._rateLimited) return false;
    return this.send(buildRequestTileMessage(slot, x, y, level));
  }
}

/** Create a new FrustaClient instance */
export function createFrustaClient(options: FrustaClientOptions): FrustaClient {
  return new FrustaClient(options);
}
