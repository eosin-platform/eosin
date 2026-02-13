/**
 * Frusta WebSocket client for connecting to the tile streaming service.
 */

import {
  type ImageDesc,
  type Viewport,
  type TileData,
  type ProgressEvent,
  type SlideCreatedEvent,
  buildOpenMessage,
  buildUpdateMessage,
  buildCloseMessage,
  buildClearCacheMessage,
  buildRequestTileMessage,
  parseTileData,
  parseProgressEvent,
  parseSlideCreated,
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
  /** Called when a progress event is received */
  onProgress?: (event: ProgressEvent) => void;
  /** Called when a slide created event is received */
  onSlideCreated?: (event: SlideCreatedEvent) => void;
  /** Called when the server signals the client is being rate limited */
  onRateLimited?: () => void;
  /** Called on error */
  onError?: (error: Event | Error) => void;
}

/** Internal record of an open slide for reconnection. */
interface TrackedSlide {
  slot: number;
  dpi: number;
  image: ImageDesc;
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
   * Key is the client-assigned slot number.
   */
  private openSlides = new Map<number, TrackedSlide>();
  /** Free slot numbers available for allocation (stack, pop from end). */
  private freeSlots: number[] = [];
  /** True while we are re-opening slides after a reconnect. */
  private _reconnecting = false;

  constructor(options: FrustaClientOptions) {
    this.options = {
      reconnectDelay: 1000,
      maxReconnectAttempts: 5,
      connectTimeout: 10000,
      onStateChange: () => {},
      onTile: () => {},
      onProgress: () => {},
      onSlideCreated: () => {},
      onRateLimited: () => {},
      onError: () => {},
      ...options,
    };
    // Initialize all 256 slots as free (push in reverse so slot 0 is popped first)
    for (let i = 255; i >= 0; i--) {
      this.freeSlots.push(i);
    }
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
    this.freeSlots = [];
    for (let i = 255; i >= 0; i--) {
      this.freeSlots.push(i);
    }
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
   * Slots are preserved — the client is authoritative for slot assignment.
   */
  private reopenTrackedSlides(): void {
    if (this.openSlides.size === 0) return;
    this._reconnecting = true;
    for (const tracked of this.openSlides.values()) {
      this.send(buildOpenMessage(tracked.slot, tracked.dpi, tracked.image));
    }
    // No server response to wait for — clear immediately
    this._reconnecting = false;
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
   * The client allocates a slot and tells the server which slot to use.
   * @param dpi Device DPI for tile scaling
   * @param image Image descriptor with UUID and dimensions
   * @returns The allocated slot number, or -1 if no slots are available
   */
  openSlide(dpi: number, image: ImageDesc): number {
    if (this.freeSlots.length === 0) return -1;
    const slot = this.freeSlots.pop()!;
    this.openSlides.set(slot, { slot, dpi, image });
    this.send(buildOpenMessage(slot, dpi, image));
    return slot;
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
   * Close a slide by its slot number.
   * @param slot The slot returned from openSlide()
   */
  closeSlide(slot: number): boolean {
    if (!this.openSlides.has(slot)) return false;
    this.openSlides.delete(slot);
    this.freeSlots.push(slot);
    return this.send(buildCloseMessage(slot));
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
