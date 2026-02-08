/**
 * Annotation API client for the WSI viewer application.
 * 
 * This module provides all HTTP operations for interacting with the
 * annotation-related endpoints on the meta backend service directly.
 * 
 * Endpoints (accessed via PUBLIC_META_ENDPOINT):
 * - GET /slides/{slide_id}/annotation-sets - List annotation sets for a slide
 * - POST /slides/{slide_id}/annotation-sets - Create annotation set
 * - PUT /annotation-sets/{set_id} - Update annotation set
 * - DELETE /annotation-sets/{set_id} - Delete annotation set
 * - GET /annotation-sets/{set_id}/annotations - List annotations in set
 * - POST /annotation-sets/{set_id}/annotations - Create annotation
 * - PUT /annotations/{annotation_id} - Update annotation
 * - DELETE /annotations/{annotation_id} - Delete annotation
 */

import { env } from '$env/dynamic/public';
import { ensureValidToken } from '$lib/auth/client';

// ============================================================================
// Types - Annotation Kinds and Geometries
// ============================================================================

/** The type of annotation geometry */
export type AnnotationKind = 'point' | 'ellipse' | 'polygon' | 'polyline' | 'mask_patch';

/** Point geometry: single pixel location */
export interface PointGeometry {
  x_level0: number;
  y_level0: number;
}

/** Ellipse geometry: center, radii, and rotation */
export interface EllipseGeometry {
  cx_level0: number;
  cy_level0: number;
  radius_x: number;
  radius_y: number;
  rotation_radians: number;
}

/** Polygon/Polyline geometry: array of [x,y] vertices in level-0 coords */
export interface PolygonGeometry {
  path: [number, number][];
}

/** Mask patch geometry: bounding box + base64 bitmask */
export interface MaskGeometry {
  x0_level0: number;
  y0_level0: number;
  width: number;
  height: number;
  encoding?: string; // "bitmask" - optional, defaults to "bitmask" on backend
  data_base64: string;
}

/** Union type for all geometry types */
export type AnnotationGeometry = PointGeometry | EllipseGeometry | PolygonGeometry | MaskGeometry;

// ============================================================================
// Types - Annotation Set
// ============================================================================

/** An annotation set (layer) */
export interface AnnotationSet {
  id: string;
  slide_id: string;
  name: string;
  description?: string;
  task_type?: string;
  locked?: boolean;
  created_at: string;
  updated_at: string;
  created_by?: string;
}

/** Request to create an annotation set */
export interface CreateAnnotationSetRequest {
  name: string;
  description?: string;
  task_type?: string;
}

/** Request to update an annotation set */
export interface UpdateAnnotationSetRequest {
  name?: string;
  description?: string;
  task_type?: string;
  locked?: boolean;
}

// ============================================================================
// Types - Annotation
// ============================================================================

/** An annotation */
export interface Annotation {
  id: string;
  annotation_set_id: string;
  kind: AnnotationKind;
  geometry: AnnotationGeometry;
  label?: string;
  label_id?: string;
  description?: string;
  properties?: Record<string, unknown>;
  created_at: string;
  updated_at: string;
  created_by?: string;
}

/** Request to create an annotation */
export interface CreateAnnotationRequest {
  kind: AnnotationKind;
  geometry: AnnotationGeometry;
  label?: string;
  label_id: string; // Required by API, use empty string for unlabeled
  description?: string;
  properties?: Record<string, unknown>;
}

/** Request to update an annotation */
export interface UpdateAnnotationRequest {
  kind?: AnnotationKind;
  geometry?: AnnotationGeometry;
  label?: string;
  label_id?: string;
  description?: string;
  properties?: Record<string, unknown>;
}

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Get the meta API base URL.
 */
function getMetaEndpoint(): string {
  const endpoint = env.PUBLIC_META_ENDPOINT;
  if (!endpoint) {
    throw new Error('PUBLIC_META_ENDPOINT is not configured');
  }
  return endpoint;
}



/**
 * Get a human-readable label for an annotation kind.
 */
export function getAnnotationLabel(kind: AnnotationKind): string {
  switch (kind) {
    case 'point': return 'Point';
    case 'ellipse': return 'Ellipse';
    case 'polygon': return 'Polygon';
    case 'polyline': return 'Polyline';
    case 'mask_patch': return 'Mask';
    default: return kind;
  }
}

/**
 * Handles API response errors.
 */
async function handleResponse<T>(response: Response): Promise<T> {
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`API error ${response.status}: ${text}`);
  }
  return response.json();
}

/**
 * Create headers for authenticated requests.
 * Automatically refreshes the token if it's close to expiry.
 */
async function createAuthHeaders(): Promise<HeadersInit> {
  const token = await ensureValidToken();
  const headers: HeadersInit = {
    'Content-Type': 'application/json',
  };
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }
  return headers;
}

// ============================================================================
// Annotation Set API
// ============================================================================

/** Response wrapper for annotation set lists */
interface ListAnnotationSetsResponse {
  items: AnnotationSet[];
}

/** Response wrapper for annotation lists */
interface ListAnnotationsResponse {
  items: Annotation[];
}

/**
 * Fetch all annotation sets for a slide.
 * Public endpoint - no authentication required.
 */
export async function fetchAnnotationSets(slideId: string): Promise<AnnotationSet[]> {
  const base = getMetaEndpoint();
  const url = `${base}/slides/${encodeURIComponent(slideId)}/annotation-sets`;
  const response = await fetch(url);
  const data = await handleResponse<ListAnnotationSetsResponse>(response);
  return data.items;
}

/**
 * Create a new annotation set for a slide.
 * Requires authentication.
 */
export async function createAnnotationSet(
  slideId: string,
  request: CreateAnnotationSetRequest
): Promise<AnnotationSet> {
  const base = getMetaEndpoint();
  const url = `${base}/slides/${encodeURIComponent(slideId)}/annotation-sets`;
  const response = await fetch(url, {
    method: 'POST',
    headers: await createAuthHeaders(),
    body: JSON.stringify(request),
  });
  return handleResponse<AnnotationSet>(response);
}

/**
 * Update an annotation set.
 * Requires authentication.
 */
export async function updateAnnotationSet(
  setId: string,
  request: UpdateAnnotationSetRequest
): Promise<AnnotationSet> {
  const base = getMetaEndpoint();
  const url = `${base}/annotation-sets/${encodeURIComponent(setId)}`;
  const response = await fetch(url, {
    method: 'PATCH',
    headers: await createAuthHeaders(),
    body: JSON.stringify(request),
  });
  return handleResponse<AnnotationSet>(response);
}

/**
 * Delete an annotation set.
 * Requires authentication.
 */
export async function deleteAnnotationSet(setId: string): Promise<void> {
  const base = getMetaEndpoint();
  const url = `${base}/annotation-sets/${encodeURIComponent(setId)}`;
  const response = await fetch(url, {
    method: 'DELETE',
    headers: await createAuthHeaders(),
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`API error ${response.status}: ${text}`);
  }
}

// ============================================================================
// Annotation API
// ============================================================================

/**
 * Fetch all annotations in an annotation set.
 * Public endpoint - no authentication required.
 */
export async function fetchAnnotations(setId: string): Promise<Annotation[]> {
  const base = getMetaEndpoint();
  // Include mask data so mask_patch annotations render properly
  const url = `${base}/annotation-sets/${encodeURIComponent(setId)}/annotations?include_mask_data=true`;
  const response = await fetch(url);
  const data = await handleResponse<ListAnnotationsResponse>(response);
  return data.items;
}

/**
 * Create a new annotation in an annotation set.
 * Requires authentication.
 */
export async function createAnnotation(
  setId: string,
  request: CreateAnnotationRequest
): Promise<Annotation> {
  const base = getMetaEndpoint();
  const url = `${base}/annotation-sets/${encodeURIComponent(setId)}/annotations`;
  const response = await fetch(url, {
    method: 'POST',
    headers: await createAuthHeaders(),
    body: JSON.stringify(request),
  });
  return handleResponse<Annotation>(response);
}

/**
 * Update an annotation.
 * Requires authentication.
 */
export async function updateAnnotation(
  annotationId: string,
  request: UpdateAnnotationRequest
): Promise<Annotation> {
  const base = getMetaEndpoint();
  const url = `${base}/annotations/${encodeURIComponent(annotationId)}`;
  const response = await fetch(url, {
    method: 'PATCH',
    headers: await createAuthHeaders(),
    body: JSON.stringify(request),
  });
  return handleResponse<Annotation>(response);
}

/**
 * Delete an annotation.
 * Requires authentication.
 */
export async function deleteAnnotation(annotationId: string): Promise<void> {
  const base = getMetaEndpoint();
  const url = `${base}/annotations/${encodeURIComponent(annotationId)}`;
  const response = await fetch(url, {
    method: 'DELETE',
    headers: await createAuthHeaders(),
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`API error ${response.status}: ${text}`);
  }
}
