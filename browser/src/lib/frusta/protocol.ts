/**
 * Frusta WebSocket protocol types and constants.
 * Mirrors the Rust protocol in frusta/src/protocol.rs
 */

/** Message header sizes */
export const DPI_SIZE = 4;
export const IMAGE_DESC_SIZE = 28;
export const UUID_SIZE = 16;
export const VIEWPORT_SIZE = 20;
export const TILE_HEADER_SIZE = 13;
/** Progress message size: 1 byte type + 16 bytes uuid + 4 bytes progress_steps + 4 bytes progress_total */
export const PROGRESS_SIZE = 25;
/** Tile request size: 1 byte type + 1 byte slot + 4 bytes x + 4 bytes y + 4 bytes level */
export const TILE_REQUEST_SIZE = 14;
/** Rate limited message size: 1 byte type only */
export const RATE_LIMITED_SIZE = 1;

/** WebSocket message types for the frusta protocol */
export enum MessageType {
  Update = 0,
  Open = 1,
  Close = 2,
  ClearCache = 3,
  Progress = 4,
  RequestTile = 5,
  RateLimited = 6,
  SlideCreated = 7,
}

/** Image descriptor sent when opening a slide */
export interface ImageDesc {
  id: Uint8Array; // 16-byte UUID
  width: number;
  height: number;
  levels: number;
}

/** Viewport state sent with Update messages */
export interface Viewport {
  x: number;
  y: number;
  width: number;
  height: number;
  zoom: number;
}

/** Tile metadata from incoming tile data */
export interface TileMeta {
  x: number;
  y: number;
  level: number;
}

/** Incoming tile with its data */
export interface TileData {
  slot: number;
  meta: TileMeta;
  data: Uint8Array;
}

/** Open response from the server */
export interface OpenResponse {
  slot: number;
  id: Uint8Array;
}

/** Progress event from the server */
export interface ProgressEvent {
  slideId: Uint8Array; // 16-byte UUID
  progressSteps: number;
  progressTotal: number;
}

/** Slide created event from the server */
export interface SlideCreatedEvent {
  id: string;
  width: number;
  height: number;
  filename: string;
  full_size: number;
  url: string;
}

/**
 * Create an Open message.
 * Format: [type: u8][dpi: f32 le][uuid: 16 bytes][width: u32 le][height: u32 le][levels: u32 le]
 */
export function buildOpenMessage(dpi: number, image: ImageDesc): ArrayBuffer {
  const buffer = new ArrayBuffer(1 + DPI_SIZE + IMAGE_DESC_SIZE);
  const view = new DataView(buffer);
  const bytes = new Uint8Array(buffer);

  view.setUint8(0, MessageType.Open);
  view.setFloat32(1, dpi, true); // little-endian
  bytes.set(image.id, 1 + DPI_SIZE);
  view.setUint32(1 + DPI_SIZE + UUID_SIZE, image.width, true);
  view.setUint32(1 + DPI_SIZE + UUID_SIZE + 4, image.height, true);
  view.setUint32(1 + DPI_SIZE + UUID_SIZE + 8, image.levels, true);

  return buffer;
}

/**
 * Create an Update message.
 * Format: [type: u8][slot: u8][x: f32 le][y: f32 le][width: u32 le][height: u32 le][zoom: f32 le]
 */
export function buildUpdateMessage(slot: number, viewport: Viewport): ArrayBuffer {
  const buffer = new ArrayBuffer(1 + 1 + VIEWPORT_SIZE);
  const view = new DataView(buffer);

  view.setUint8(0, MessageType.Update);
  view.setUint8(1, slot);
  view.setFloat32(2, viewport.x, true);
  view.setFloat32(6, viewport.y, true);
  view.setUint32(10, viewport.width, true);
  view.setUint32(14, viewport.height, true);
  view.setFloat32(18, viewport.zoom, true);

  return buffer;
}

/**
 * Create a Close message.
 * Format: [type: u8][uuid: 16 bytes]
 */
export function buildCloseMessage(id: Uint8Array): ArrayBuffer {
  const buffer = new ArrayBuffer(1 + UUID_SIZE);
  const bytes = new Uint8Array(buffer);

  bytes[0] = MessageType.Close;
  bytes.set(id, 1);

  return buffer;
}

/**
 * Create a ClearCache message.
 * Format: [type: u8][slot: u8]
 */
export function buildClearCacheMessage(slot: number): ArrayBuffer {
  const buffer = new ArrayBuffer(2);
  const view = new DataView(buffer);

  view.setUint8(0, MessageType.ClearCache);
  view.setUint8(1, slot);

  return buffer;
}

/**
 * Create a RequestTile message for requesting a specific tile.
 * Format: [type: u8][slot: u8][x: u32 le][y: u32 le][level: u32 le]
 */
export function buildRequestTileMessage(slot: number, x: number, y: number, level: number): ArrayBuffer {
  const buffer = new ArrayBuffer(TILE_REQUEST_SIZE);
  const view = new DataView(buffer);

  view.setUint8(0, MessageType.RequestTile);
  view.setUint8(1, slot);
  view.setUint32(2, x, true);
  view.setUint32(6, y, true);
  view.setUint32(10, level, true);

  return buffer;
}

/**
 * Parse an Open response message.
 * Format: [type: u8][slot: u8][uuid: 16 bytes]
 */
export function parseOpenResponse(data: ArrayBuffer): OpenResponse | null {
  const bytes = new Uint8Array(data);
  if (bytes.length < 2 + UUID_SIZE) return null;
  if (bytes[0] !== MessageType.Open) return null;

  return {
    slot: bytes[1],
    id: bytes.slice(2, 2 + UUID_SIZE),
  };
}

/**
 * Parse incoming tile data.
 * Format: [slot: u8][x: u32 le][y: u32 le][level: u32 le][data: bytes]
 */
export function parseTileData(data: ArrayBuffer): TileData | null {
  if (data.byteLength < TILE_HEADER_SIZE) return null;

  const view = new DataView(data);
  const bytes = new Uint8Array(data);

  return {
    slot: view.getUint8(0),
    meta: {
      x: view.getUint32(1, true),
      y: view.getUint32(5, true),
      level: view.getUint32(9, true),
    },
    data: bytes.slice(TILE_HEADER_SIZE),
  };
}

/**
 * Check if a binary message is an Open response (starts with MessageType.Open)
 */
export function isOpenResponse(data: ArrayBuffer): boolean {
  const bytes = new Uint8Array(data);
  // Open responses have a fixed size.
  // Tile frames begin with a slot byte (no type byte), so require exact length
  // to avoid misclassifying tile frames for slot==MessageType.Open.
  return bytes.length === 2 + UUID_SIZE && bytes[0] === MessageType.Open;
}

/**
 * Check if a binary message is a Progress event (starts with MessageType.Progress)
 */
export function isProgressEvent(data: ArrayBuffer): boolean {
  const bytes = new Uint8Array(data);
  // Progress events have a fixed size.
  // Tile frames begin with a slot byte (no type byte), so require exact length
  // to avoid misclassifying tile frames for slot==MessageType.Progress.
  return bytes.length === PROGRESS_SIZE && bytes[0] === MessageType.Progress;
}

/**
 * Parse a Progress event message.
 * Format: [type: u8][uuid: 16 bytes][progress_steps: i32 le][progress_total: i32 le]
 */
export function parseProgressEvent(data: ArrayBuffer): ProgressEvent | null {
  if (data.byteLength < PROGRESS_SIZE) return null;
  
  const bytes = new Uint8Array(data);
  if (bytes[0] !== MessageType.Progress) return null;
  
  const view = new DataView(data);
  return {
    slideId: bytes.slice(1, 1 + UUID_SIZE),
    progressSteps: view.getInt32(1 + UUID_SIZE, true),
    progressTotal: view.getInt32(1 + UUID_SIZE + 4, true),
  };
}

/**
 * Check if a binary message is a RateLimited notification (starts with MessageType.RateLimited)
 */
export function isRateLimited(data: ArrayBuffer): boolean {
  const bytes = new Uint8Array(data);
  // RateLimited notifications are a single byte.
  // Require exact length to avoid misclassifying tile frames for slot==MessageType.RateLimited.
  return bytes.length === RATE_LIMITED_SIZE && bytes[0] === MessageType.RateLimited;
}

/**
 * Check if a binary message is a SlideCreated event (starts with MessageType.SlideCreated)
 */
export function isSlideCreated(data: ArrayBuffer): boolean {
  const bytes = new Uint8Array(data);
  // SlideCreated messages are variable-length (JSON payload).
  // Must be at least 2 bytes (type + minimal JSON).
  return bytes.length >= 2 && bytes[0] === MessageType.SlideCreated;
}

/**
 * Parse a SlideCreated event message.
 * Format: [type: u8][json payload]
 */
export function parseSlideCreated(data: ArrayBuffer): SlideCreatedEvent | null {
  const bytes = new Uint8Array(data);
  if (bytes.length < 2 || bytes[0] !== MessageType.SlideCreated) return null;

  try {
    const jsonStr = new TextDecoder().decode(bytes.slice(1));
    return JSON.parse(jsonStr) as SlideCreatedEvent;
  } catch {
    return null;
  }
}
