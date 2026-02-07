/// <reference types="@webgpu/types" />

/**
 * GPU-Accelerated Tile Processing Worker
 * 
 * This web worker processes image tiles using either WebGPU (when available and enabled)
 * or falls back to CPU processing. It supports concurrent tile processing with a
 * managed job queue and backpressure control.
 * 
 * Usage:
 * const worker = new Worker(new URL("./gpuTileWorker.ts", import.meta.url), { type: "module" });
 * worker.postMessage({ type: "init", hardwareAccelerationEnabled: true });
 * worker.postMessage({ type: "processTile", id: "tile-1", width: 256, height: 256, pixels: data });
 */

// ============================================================================
// Message Types
// ============================================================================

/** Initialization message sent from main thread to enable/disable hardware acceleration */
export type WorkerInitMessage = {
  type: "init";
  hardwareAccelerationEnabled: boolean;
};

/** Request to process a tile with RGBA pixel data */
export type WorkerProcessTileMessage = {
  type: "processTile";
  id: string;
  width: number;
  height: number;
  pixels: Uint8ClampedArray;
};

/** Successful tile processing result */
export type WorkerTileResultMessage = {
  type: "tileResult";
  id: string;
  width: number;
  height: number;
  pixels: Uint8ClampedArray;
};

/** Error message for failed processing */
export type WorkerErrorMessage = {
  type: "error";
  id?: string;
  message: string;
};

export type WorkerInboundMessage = WorkerInitMessage | WorkerProcessTileMessage;
export type WorkerOutboundMessage = WorkerTileResultMessage | WorkerErrorMessage;

// ============================================================================
// Worker State
// ============================================================================

/** Whether hardware acceleration is enabled by user settings */
let hardwareAccelerationEnabled = false;

/** Whether WebGPU was successfully initialized */
let gpuAvailable = false;

/** WebGPU device reference */
let gpuDevice: GPUDevice | null = null;

/** Compute pipeline for tile processing */
let computePipeline: GPUComputePipeline | null = null;

/** Bind group layout for the compute shader */
let bindGroupLayout: GPUBindGroupLayout | null = null;

/** Whether we're currently initializing WebGPU */
let isInitializing = false;

/** Promise that resolves when initialization is complete */
let initPromise: Promise<void> | null = null;

// ============================================================================
// Job Queue State
// ============================================================================

/** Maximum number of concurrent jobs to prevent GPU command flooding */
const MAX_CONCURRENT_JOBS = 3;

/** Currently processing job count */
let activeJobCount = 0;

/** Pending job queue */
interface PendingJob {
  id: string;
  width: number;
  height: number;
  pixels: Uint8ClampedArray;
}
const pendingJobs: PendingJob[] = [];

// ============================================================================
// Reusable GPU Buffers (for performance optimization)
// ============================================================================

/** Cached input buffer - reused across jobs when size allows */
let cachedInputBuffer: GPUBuffer | null = null;
let cachedInputBufferSize = 0;

/** Cached output buffer - reused across jobs when size allows */
let cachedOutputBuffer: GPUBuffer | null = null;
let cachedOutputBufferSize = 0;

/** Staging buffer for reading back results */
let cachedStagingBuffer: GPUBuffer | null = null;
let cachedStagingBufferSize = 0;

// ============================================================================
// WGSL Compute Shader
// ============================================================================

/**
 * WGSL shader for per-pixel image processing.
 * 
 * This shader implements:
 * - Contrast adjustment using S-curve
 * - Brightness adjustment
 * - Luminance-preserving sharpening approximation
 * 
 * The shader operates on RGBA pixels stored as u32 values (packed RGBA).
 * Workgroup size of 8x8 = 64 threads provides good occupancy on most GPUs.
 */
const TILE_PROCESSING_SHADER = /* wgsl */ `
  // Input/output storage buffers for pixel data
  @group(0) @binding(0) var<storage, read> inputPixels: array<u32>;
  @group(0) @binding(1) var<storage, read_write> outputPixels: array<u32>;
  
  // Uniforms for image dimensions and processing parameters
  struct Params {
    width: u32,
    height: u32,
    contrastAmount: f32,
    brightnessAmount: f32,
    sharpenAmount: f32,
    padding1: f32,
    padding2: f32,
    padding3: f32,
  }
  @group(0) @binding(2) var<uniform> params: Params;

  // Unpack RGBA from u32 (stored as ABGR in little-endian)
  fn unpackRGBA(packed: u32) -> vec4<f32> {
    let r = f32((packed >> 0u) & 0xFFu) / 255.0;
    let g = f32((packed >> 8u) & 0xFFu) / 255.0;
    let b = f32((packed >> 16u) & 0xFFu) / 255.0;
    let a = f32((packed >> 24u) & 0xFFu) / 255.0;
    return vec4<f32>(r, g, b, a);
  }

  // Pack RGBA to u32
  fn packRGBA(color: vec4<f32>) -> u32 {
    let r = u32(clamp(color.r * 255.0, 0.0, 255.0));
    let g = u32(clamp(color.g * 255.0, 0.0, 255.0));
    let b = u32(clamp(color.b * 255.0, 0.0, 255.0));
    let a = u32(clamp(color.a * 255.0, 0.0, 255.0));
    return (a << 24u) | (b << 16u) | (g << 8u) | r;
  }

  // Compute luminance using Rec. 601 weights
  fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.299, 0.587, 0.114));
  }

  // Apply contrast using S-curve (sigmoid-like)
  fn applyContrast(color: vec3<f32>, amount: f32) -> vec3<f32> {
    if (amount == 0.0) {
      return color;
    }
    // Map amount from [-1, 1] to contrast factor
    let factor = 1.0 + amount;
    let adjusted = (color - 0.5) * factor + 0.5;
    return clamp(adjusted, vec3<f32>(0.0), vec3<f32>(1.0));
  }

  // Apply brightness adjustment
  fn applyBrightness(color: vec3<f32>, amount: f32) -> vec3<f32> {
    return clamp(color + amount, vec3<f32>(0.0), vec3<f32>(1.0));
  }

  // Get pixel at coordinates with bounds checking
  fn getPixel(x: i32, y: i32) -> vec4<f32> {
    let cx = clamp(x, 0i, i32(params.width) - 1i);
    let cy = clamp(y, 0i, i32(params.height) - 1i);
    let idx = u32(cy) * params.width + u32(cx);
    return unpackRGBA(inputPixels[idx]);
  }

  // Simple 3x3 sharpening using luminance-based unsharp mask
  fn applySharpen(x: i32, y: i32, center: vec4<f32>, amount: f32) -> vec4<f32> {
    if (amount <= 0.0) {
      return center;
    }

    // Sample 3x3 neighborhood for blur
    let p00 = getPixel(x - 1, y - 1);
    let p01 = getPixel(x, y - 1);
    let p02 = getPixel(x + 1, y - 1);
    let p10 = getPixel(x - 1, y);
    let p11 = center;
    let p12 = getPixel(x + 1, y);
    let p20 = getPixel(x - 1, y + 1);
    let p21 = getPixel(x, y + 1);
    let p22 = getPixel(x + 1, y + 1);

    // Gaussian-like 3x3 kernel weights: [1,2,1; 2,4,2; 1,2,1] / 16
    let blurred = (
      p00.rgb * 1.0 + p01.rgb * 2.0 + p02.rgb * 1.0 +
      p10.rgb * 2.0 + p11.rgb * 4.0 + p12.rgb * 2.0 +
      p20.rgb * 1.0 + p21.rgb * 2.0 + p22.rgb * 1.0
    ) / 16.0;

    // Compute luminance of original and blurred
    let origLum = luminance(center.rgb);
    let blurLum = luminance(blurred);

    // High-frequency detail in luminance domain
    let detail = origLum - blurLum;

    // Sharpened luminance
    let sharpLum = origLum + amount * detail;

    // Scale RGB to match new luminance (preserve color ratios)
    var result = center.rgb;
    if (origLum > 0.001) {
      let scale = clamp(sharpLum / origLum, 0.0, 2.0);
      result = clamp(center.rgb * scale, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    return vec4<f32>(result, center.a);
  }

  @compute @workgroup_size(8, 8)
  fn main(@builtin(global_invocation_id) globalId: vec3<u32>) {
    let x = globalId.x;
    let y = globalId.y;

    // Bounds check for tiles that don't align perfectly with workgroup size
    if (x >= params.width || y >= params.height) {
      return;
    }

    let idx = y * params.width + x;
    var pixel = unpackRGBA(inputPixels[idx]);

    // Apply processing pipeline
    pixel = vec4<f32>(
      applyContrast(pixel.rgb, params.contrastAmount),
      pixel.a
    );

    pixel = vec4<f32>(
      applyBrightness(pixel.rgb, params.brightnessAmount),
      pixel.a
    );

    pixel = applySharpen(i32(x), i32(y), pixel, params.sharpenAmount);

    // Write result
    outputPixels[idx] = packRGBA(pixel);
  }
`;

// ============================================================================
// WebGPU Initialization
// ============================================================================

/**
 * Initialize WebGPU resources.
 * 
 * This function checks for WebGPU availability, requests an adapter and device,
 * compiles the compute shader, and creates the pipeline. On failure, it logs
 * the error and marks GPU as unavailable for graceful CPU fallback.
 * 
 * @returns Promise that resolves when initialization is complete
 */
async function initWebGPU(): Promise<void> {
  if (isInitializing && initPromise) {
    return initPromise;
  }

  isInitializing = true;
  initPromise = (async () => {
    try {
      // Check if WebGPU is available in this environment
      if (typeof navigator === "undefined" || !("gpu" in navigator)) {
        console.log("[gpuTileWorker] WebGPU not available in this environment");
        gpuAvailable = false;
        return;
      }

      const gpu = (navigator as Navigator & { gpu?: GPU }).gpu;
      if (!gpu) {
        console.log("[gpuTileWorker] navigator.gpu is undefined");
        gpuAvailable = false;
        return;
      }

      // Request adapter
      const adapter = await gpu.requestAdapter({
        powerPreference: "high-performance",
      });

      if (!adapter) {
        console.log("[gpuTileWorker] Failed to get WebGPU adapter");
        gpuAvailable = false;
        return;
      }

      // Request device
      gpuDevice = await adapter.requestDevice({
        requiredFeatures: [],
        requiredLimits: {},
      });

      if (!gpuDevice) {
        console.log("[gpuTileWorker] Failed to get WebGPU device");
        gpuAvailable = false;
        return;
      }

      // Handle device loss
      gpuDevice.lost.then((info) => {
        console.error("[gpuTileWorker] WebGPU device lost:", info.message);
        gpuAvailable = false;
        gpuDevice = null;
        computePipeline = null;
        bindGroupLayout = null;
        // Clear cached buffers
        cachedInputBuffer = null;
        cachedOutputBuffer = null;
        cachedStagingBuffer = null;
      });

      // Create shader module
      const shaderModule = gpuDevice.createShaderModule({
        label: "tile-processing-shader",
        code: TILE_PROCESSING_SHADER,
      });

      // Create bind group layout
      bindGroupLayout = gpuDevice.createBindGroupLayout({
        label: "tile-processing-bind-group-layout",
        entries: [
          {
            binding: 0,
            visibility: GPUShaderStage.COMPUTE,
            buffer: { type: "read-only-storage" },
          },
          {
            binding: 1,
            visibility: GPUShaderStage.COMPUTE,
            buffer: { type: "storage" },
          },
          {
            binding: 2,
            visibility: GPUShaderStage.COMPUTE,
            buffer: { type: "uniform" },
          },
        ],
      });

      // Create pipeline layout
      const pipelineLayout = gpuDevice.createPipelineLayout({
        label: "tile-processing-pipeline-layout",
        bindGroupLayouts: [bindGroupLayout],
      });

      // Create compute pipeline
      computePipeline = gpuDevice.createComputePipeline({
        label: "tile-processing-pipeline",
        layout: pipelineLayout,
        compute: {
          module: shaderModule,
          entryPoint: "main",
        },
      });

      gpuAvailable = true;
      console.log("[gpuTileWorker] WebGPU initialized successfully");
    } catch (error) {
      console.error("[gpuTileWorker] WebGPU initialization failed:", error);
      gpuAvailable = false;
      gpuDevice = null;
      computePipeline = null;
      bindGroupLayout = null;
    } finally {
      isInitializing = false;
    }
  })();

  return initPromise;
}

// ============================================================================
// GPU Buffer Management
// ============================================================================

/**
 * Get or create an input buffer of appropriate size.
 * Reuses existing buffer if large enough, otherwise creates a new one.
 */
function getInputBuffer(device: GPUDevice, requiredSize: number): GPUBuffer {
  if (cachedInputBuffer && cachedInputBufferSize >= requiredSize) {
    return cachedInputBuffer;
  }

  // Destroy old buffer
  if (cachedInputBuffer) {
    cachedInputBuffer.destroy();
  }

  // Allocate with some headroom to reduce reallocations
  const allocSize = Math.max(requiredSize, requiredSize * 1.5);
  cachedInputBuffer = device.createBuffer({
    label: "tile-input-buffer",
    size: allocSize,
    usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
  });
  cachedInputBufferSize = allocSize;

  return cachedInputBuffer;
}

/**
 * Get or create an output buffer of appropriate size.
 */
function getOutputBuffer(device: GPUDevice, requiredSize: number): GPUBuffer {
  if (cachedOutputBuffer && cachedOutputBufferSize >= requiredSize) {
    return cachedOutputBuffer;
  }

  if (cachedOutputBuffer) {
    cachedOutputBuffer.destroy();
  }

  const allocSize = Math.max(requiredSize, requiredSize * 1.5);
  cachedOutputBuffer = device.createBuffer({
    label: "tile-output-buffer",
    size: allocSize,
    usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC,
  });
  cachedOutputBufferSize = allocSize;

  return cachedOutputBuffer;
}

/**
 * Get or create a staging buffer for reading results back to CPU.
 */
function getStagingBuffer(device: GPUDevice, requiredSize: number): GPUBuffer {
  if (cachedStagingBuffer && cachedStagingBufferSize >= requiredSize) {
    return cachedStagingBuffer;
  }

  if (cachedStagingBuffer) {
    cachedStagingBuffer.destroy();
  }

  const allocSize = Math.max(requiredSize, requiredSize * 1.5);
  cachedStagingBuffer = device.createBuffer({
    label: "tile-staging-buffer",
    size: allocSize,
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });
  cachedStagingBufferSize = allocSize;

  return cachedStagingBuffer;
}

// ============================================================================
// GPU Tile Processing
// ============================================================================

/**
 * Process a tile using WebGPU compute shader.
 * 
 * This function uploads pixel data to GPU buffers, dispatches the compute
 * shader with appropriate workgroup counts, reads back the results, and
 * returns the processed pixel data.
 * 
 * @param pixels - RGBA pixel data as Uint8ClampedArray
 * @param width - Tile width in pixels
 * @param height - Tile height in pixels
 * @returns Promise resolving to processed pixel data
 */
async function processTileWithWebGPU(
  pixels: Uint8ClampedArray,
  width: number,
  height: number
): Promise<Uint8ClampedArray> {
  if (!gpuDevice || !computePipeline || !bindGroupLayout) {
    throw new Error("WebGPU not initialized");
  }

  const numPixels = width * height;
  const bufferSize = numPixels * 4; // 4 bytes per pixel (RGBA)

  // Get or create buffers
  const inputBuffer = getInputBuffer(gpuDevice, bufferSize);
  const outputBuffer = getOutputBuffer(gpuDevice, bufferSize);

  // Create uniform buffer for parameters (must be new each dispatch for different tiles)
  // Params struct: width, height, contrastAmount, brightnessAmount, sharpenAmount, padding[3]
  const uniformBuffer = gpuDevice.createBuffer({
    label: "tile-params-uniform",
    size: 32, // 8 * 4 bytes = 32 bytes (aligned to 16 bytes)
    usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
  });

  // Processing parameters (can be extended to receive from main thread)
  const contrastAmount = 0.0; // No contrast adjustment by default
  const brightnessAmount = 0.0; // No brightness adjustment by default  
  const sharpenAmount = 0.3; // Light sharpening for histology clarity

  const uniformData = new Float32Array([
    width,
    height,
    contrastAmount,
    brightnessAmount,
    sharpenAmount,
    0, // padding
    0, // padding
    0, // padding
  ]);
  // Set first two as u32 (width/height)
  const uniformDataBytes = new ArrayBuffer(32);
  const uniformView = new DataView(uniformDataBytes);
  uniformView.setUint32(0, width, true);
  uniformView.setUint32(4, height, true);
  uniformView.setFloat32(8, contrastAmount, true);
  uniformView.setFloat32(12, brightnessAmount, true);
  uniformView.setFloat32(16, sharpenAmount, true);
  uniformView.setFloat32(20, 0, true);
  uniformView.setFloat32(24, 0, true);
  uniformView.setFloat32(28, 0, true);

  // Upload pixel data (ensure we have a plain ArrayBuffer for WebGPU)
  const pixelData = new Uint8Array(pixels.buffer as ArrayBuffer, pixels.byteOffset, pixels.byteLength);
  gpuDevice.queue.writeBuffer(inputBuffer, 0, pixelData);
  gpuDevice.queue.writeBuffer(uniformBuffer, 0, new Uint8Array(uniformDataBytes));

  // Create bind group
  const bindGroup = gpuDevice.createBindGroup({
    label: "tile-processing-bind-group",
    layout: bindGroupLayout,
    entries: [
      { binding: 0, resource: { buffer: inputBuffer, size: bufferSize } },
      { binding: 1, resource: { buffer: outputBuffer, size: bufferSize } },
      { binding: 2, resource: { buffer: uniformBuffer } },
    ],
  });

  // Create command encoder and dispatch compute
  const commandEncoder = gpuDevice.createCommandEncoder({
    label: "tile-processing-command",
  });

  const computePass = commandEncoder.beginComputePass({
    label: "tile-processing-pass",
  });

  computePass.setPipeline(computePipeline);
  computePass.setBindGroup(0, bindGroup);

  // Calculate workgroup dispatch counts (8x8 workgroup size)
  const workgroupsX = Math.ceil(width / 8);
  const workgroupsY = Math.ceil(height / 8);
  computePass.dispatchWorkgroups(workgroupsX, workgroupsY);

  computePass.end();

  // We need to copy output to a staging buffer for readback
  // Create a new staging buffer for this operation to avoid conflicts
  const stagingBuffer = gpuDevice.createBuffer({
    label: "tile-staging-buffer-temp",
    size: bufferSize,
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });

  commandEncoder.copyBufferToBuffer(outputBuffer, 0, stagingBuffer, 0, bufferSize);

  // Submit commands
  gpuDevice.queue.submit([commandEncoder.finish()]);

  // Wait for GPU to complete and read back results
  await stagingBuffer.mapAsync(GPUMapMode.READ);
  const resultArrayBuffer = stagingBuffer.getMappedRange();
  const resultData = new Uint8ClampedArray(resultArrayBuffer.slice(0));
  stagingBuffer.unmap();

  // Clean up temporary buffers
  uniformBuffer.destroy();
  stagingBuffer.destroy();

  return resultData;
}

// ============================================================================
// CPU Tile Processing (Fallback)
// ============================================================================

/**
 * Process a tile using CPU.
 * 
 * This is the fallback path when WebGPU is not available or disabled.
 * Implements the same processing operations as the GPU shader:
 * - Contrast adjustment
 * - Brightness adjustment
 * - Luminance-preserving sharpening
 * 
 * @param pixels - RGBA pixel data as Uint8ClampedArray
 * @param width - Tile width in pixels
 * @param height - Tile height in pixels
 * @returns Promise resolving to processed pixel data
 */
async function processTileCPU(
  pixels: Uint8ClampedArray,
  width: number,
  height: number
): Promise<Uint8ClampedArray> {
  // Create a copy to avoid modifying input
  const result = new Uint8ClampedArray(pixels);

  const contrastAmount = 0.0;
  const brightnessAmount = 0.0;
  const sharpenAmount = 0.3;

  const numPixels = width * height;

  // Apply contrast and brightness (in-place on result)
  for (let i = 0; i < numPixels; i++) {
    const offset = i * 4;
    let r = result[offset] / 255;
    let g = result[offset + 1] / 255;
    let b = result[offset + 2] / 255;

    // Contrast
    if (contrastAmount !== 0) {
      const factor = 1 + contrastAmount;
      r = Math.max(0, Math.min(1, (r - 0.5) * factor + 0.5));
      g = Math.max(0, Math.min(1, (g - 0.5) * factor + 0.5));
      b = Math.max(0, Math.min(1, (b - 0.5) * factor + 0.5));
    }

    // Brightness
    if (brightnessAmount !== 0) {
      r = Math.max(0, Math.min(1, r + brightnessAmount));
      g = Math.max(0, Math.min(1, g + brightnessAmount));
      b = Math.max(0, Math.min(1, b + brightnessAmount));
    }

    result[offset] = Math.round(r * 255);
    result[offset + 1] = Math.round(g * 255);
    result[offset + 2] = Math.round(b * 255);
  }

  // Apply sharpening if enabled
  if (sharpenAmount > 0) {
    applyUnsharpMaskLuminanceCPU(result, width, height, sharpenAmount);
  }

  // Return result wrapped in a resolved promise for consistent async API
  return result;
}

/**
 * Apply luminance-only unsharp mask sharpening (CPU implementation).
 * 
 * This matches the GPU shader's sharpening algorithm for consistent results
 * between GPU and CPU code paths.
 */
function applyUnsharpMaskLuminanceCPU(
  pixels: Uint8ClampedArray,
  width: number,
  height: number,
  amount: number
): void {
  if (amount <= 0) return;

  const wR = 0.299;
  const wG = 0.587;
  const wB = 0.114;

  const numPixels = width * height;

  // Step 1: Compute original luminance
  const luminance = new Float32Array(numPixels);
  for (let i = 0; i < numPixels; i++) {
    const offset = i * 4;
    luminance[i] =
      wR * pixels[offset] / 255 +
      wG * pixels[offset + 1] / 255 +
      wB * pixels[offset + 2] / 255;
  }

  // Step 2: Compute blurred luminance (3x3 Gaussian-like kernel)
  const kernel = [1, 2, 1, 2, 4, 2, 1, 2, 1];
  const blurred = new Float32Array(numPixels);

  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let acc = 0;
      let wSum = 0;

      for (let ky = -1; ky <= 1; ky++) {
        for (let kx = -1; kx <= 1; kx++) {
          const ix = Math.max(0, Math.min(width - 1, x + kx));
          const iy = Math.max(0, Math.min(height - 1, y + ky));
          const weight = kernel[(ky + 1) * 3 + (kx + 1)];
          acc += luminance[iy * width + ix] * weight;
          wSum += weight;
        }
      }

      blurred[y * width + x] = acc / wSum;
    }
  }

  // Step 3: Apply unsharp mask and scale RGB
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const idx = y * width + x;
      const offset = idx * 4;

      const origLum = luminance[idx];
      const blurLum = blurred[idx];
      const detail = origLum - blurLum;

      let sharpLum = origLum + amount * detail;
      sharpLum = Math.max(0, Math.min(1, sharpLum));

      const r = pixels[offset] / 255;
      const g = pixels[offset + 1] / 255;
      const b = pixels[offset + 2] / 255;

      if (origLum > 0.001) {
        const scale = Math.max(0, Math.min(2, sharpLum / origLum));
        pixels[offset] = Math.round(Math.max(0, Math.min(1, r * scale)) * 255);
        pixels[offset + 1] = Math.round(Math.max(0, Math.min(1, g * scale)) * 255);
        pixels[offset + 2] = Math.round(Math.max(0, Math.min(1, b * scale)) * 255);
      }
    }
  }
}

// ============================================================================
// Unified Tile Processing
// ============================================================================

/**
 * Process a tile using the best available path (GPU or CPU).
 * 
 * This function routes to GPU processing if:
 * 1. Hardware acceleration is enabled by user settings
 * 2. WebGPU was successfully initialized
 * 
 * Otherwise, it falls back to CPU processing.
 * 
 * @param pixels - RGBA pixel data as Uint8ClampedArray
 * @param width - Tile width in pixels
 * @param height - Tile height in pixels
 * @returns Promise resolving to processed pixel data
 */
async function processTile(
  pixels: Uint8ClampedArray,
  width: number,
  height: number
): Promise<Uint8ClampedArray> {
  // Check if GPU path is available
  if (hardwareAccelerationEnabled && gpuAvailable && gpuDevice && computePipeline) {
    try {
      return await processTileWithWebGPU(pixels, width, height);
    } catch (error) {
      // Log error and fall back to CPU
      console.warn("[gpuTileWorker] GPU processing failed, falling back to CPU:", error);
      // Mark GPU as unavailable for future jobs in this session
      gpuAvailable = false;
    }
  }

  // CPU fallback
  return processTileCPU(pixels, width, height);
}

// ============================================================================
// Job Queue Management
// ============================================================================

/**
 * Process a single job from the queue.
 * 
 * Handles the full lifecycle of a job: processing, result posting, and
 * error handling. After completion (success or failure), triggers processing
 * of the next queued job.
 */
async function processJob(job: PendingJob): Promise<void> {
  try {
    const result = await processTile(job.pixels, job.width, job.height);

    const response: WorkerTileResultMessage = {
      type: "tileResult",
      id: job.id,
      width: job.width,
      height: job.height,
      pixels: result,
    };

    // Transfer the result buffer for zero-copy communication
    self.postMessage(response, { transfer: [result.buffer] });
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    console.error(`[gpuTileWorker] Job ${job.id} failed:`, errorMessage);

    const errorResponse: WorkerErrorMessage = {
      type: "error",
      id: job.id,
      message: errorMessage,
    };

    self.postMessage(errorResponse);
  } finally {
    activeJobCount--;
    processNextJobs();
  }
}

/**
 * Start processing jobs from the queue up to the concurrency limit.
 * 
 * This is called when new jobs arrive or when existing jobs complete.
 * It ensures we're always processing up to MAX_CONCURRENT_JOBS jobs
 * in parallel without overwhelming the GPU command queue.
 */
function processNextJobs(): void {
  while (activeJobCount < MAX_CONCURRENT_JOBS && pendingJobs.length > 0) {
    const job = pendingJobs.shift()!;
    activeJobCount++;
    // Don't await - let jobs run concurrently
    processJob(job);
  }
}

/**
 * Queue a new tile processing job.
 * 
 * Adds the job to the pending queue and triggers processing if capacity
 * is available. Jobs are processed in FIFO order with up to
 * MAX_CONCURRENT_JOBS running in parallel.
 */
function queueJob(
  id: string,
  width: number,
  height: number,
  pixels: Uint8ClampedArray
): void {
  pendingJobs.push({ id, width, height, pixels });
  processNextJobs();
}

// ============================================================================
// Message Handler
// ============================================================================

/**
 * Handle incoming messages from the main thread.
 * 
 * Supports two message types:
 * - "init": Initialize worker with hardware acceleration preference
 * - "processTile": Queue a tile for processing
 */
self.addEventListener("message", async (event: MessageEvent<WorkerInboundMessage>) => {
  const message = event.data;

  switch (message.type) {
    case "init": {
      hardwareAccelerationEnabled = message.hardwareAccelerationEnabled;
      console.log(
        `[gpuTileWorker] Initializing with hardwareAccelerationEnabled=${hardwareAccelerationEnabled}`
      );

      if (hardwareAccelerationEnabled) {
        // Attempt to initialize WebGPU
        await initWebGPU();

        if (gpuAvailable) {
          console.log("[gpuTileWorker] Ready with GPU acceleration");
        } else {
          console.log("[gpuTileWorker] GPU not available, using CPU fallback");
        }
      } else {
        console.log("[gpuTileWorker] Hardware acceleration disabled, using CPU");
        gpuAvailable = false;
      }
      break;
    }

    case "processTile": {
      // Validate required fields
      if (!message.id || !message.width || !message.height || !message.pixels) {
        const errorResponse: WorkerErrorMessage = {
          type: "error",
          id: message.id,
          message: "Invalid processTile message: missing required fields",
        };
        self.postMessage(errorResponse);
        return;
      }

      // Ensure pixels is a Uint8ClampedArray
      let pixels: Uint8ClampedArray;
      if (message.pixels instanceof Uint8ClampedArray) {
        pixels = message.pixels;
      } else if (ArrayBuffer.isView(message.pixels)) {
        pixels = new Uint8ClampedArray(
          (message.pixels as ArrayBufferView).buffer,
          (message.pixels as ArrayBufferView).byteOffset,
          (message.pixels as ArrayBufferView).byteLength
        );
      } else {
        const errorResponse: WorkerErrorMessage = {
          type: "error",
          id: message.id,
          message: "Invalid pixel data format",
        };
        self.postMessage(errorResponse);
        return;
      }

      // Queue the job
      queueJob(message.id, message.width, message.height, pixels);
      break;
    }

    default: {
      console.warn("[gpuTileWorker] Unknown message type:", (message as { type: string }).type);
    }
  }
});

// Log worker startup
console.log("[gpuTileWorker] Worker loaded and ready for init message");
