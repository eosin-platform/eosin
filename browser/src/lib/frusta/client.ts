/**
 * Frusta WebSocket client for connecting to the tile streaming service.
 */

import {
  type ImageDesc,
  type Viewport,
  type TileData,
  type OpenResponse,
  type ProgressEvent,
  buildOpenMessage,
  buildUpdateMessage,
  buildCloseMessage,
  buildClearCacheMessage,
  buildRequestTileMessage,
  parseOpenResponse,
  parseTileData,
  parseProgressEvent,
  isOpenResponse,
  isProgressEvent,
  isRateLimited,
} from './protocol';

export type ConnectionState = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface FrustaClientOptions {
  /** WebSocket URL (e.g., "ws://localhost:8080/ws") */
  url: string;
  /** Reconnect delay in milliseconds (default: 1000) */
  reconnectDelay?: number;
  /** Maximum reconnect attempts (default: 5, 0 = infinite) */
  maxReconnectAttempts?: number;
  /** Called when connection state changes */
  onStateChange?: (state: ConnectionState) => void;
  /** Called when a tile is received */
  onTile?: (tile: TileData) => void;
  /** Called when an open response is received */
  onOpenResponse?: (response: OpenResponse) => void;
  /** Called when a progress event is received */
  onProgress?: (event: ProgressEvent) => void;
  /** Called when the server signals the client is being rate limited */
  onRateLimited?: () => void;
  /** Called on error */
  onError?: (error: Event | Error) => void;
}

export class FrustaClient {
  private ws: WebSocket | null = null;
  private options: Required<FrustaClientOptions>;
  private reconnectAttempts = 0;
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private _state: ConnectionState = 'disconnected';
  private intentionalClose = false;
  private _rateLimited = false;
  private rateLimitCooldownTimeout: ReturnType<typeof setTimeout> | null = null;

  constructor(options: FrustaClientOptions) {
    this.options = {
      reconnectDelay: 1000,
      maxReconnectAttempts: 5,
      onStateChange: () => {},
      onTile: () => {},
      onOpenResponse: () => {},
      onProgress: () => {},
      onRateLimited: () => {},
      onError: () => {},
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

      this.ws.onopen = () => {
        this.reconnectAttempts = 0;
        this.setState('connected');
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
    if (this.rateLimitCooldownTimeout) {
      clearTimeout(this.rateLimitCooldownTimeout);
      this.rateLimitCooldownTimeout = null;
    }
    this._rateLimited = false;
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    this.setState('disconnected');
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
    return this.send(buildOpenMessage(dpi, image));
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
