/**
 * Worker Pool Manager for Tile Processing
 * 
 * Manages a pool of Web Workers for off-main-thread stain normalization
 * and enhancement. Features:
 * - Round-robin task distribution across workers
 * - Priority queue with viewport-center tiles processed first
 * - Automatic cancellation of outdated requests during zoom
 * - Debounced processing to avoid overwhelming workers during rapid zoom
 */

import type { StainNormalizationMode, NormalizationParams } from './stainNormalization';
import type { StainEnhancementMode } from '$lib/stores/settings';
import type { ProcessTileRequest, ProcessTileResponse, CancelRequest } from './processingWorker';

// Vite worker import
import ProcessingWorker from './processingWorker?worker';

export interface ProcessingTask {
  id: string;
  imageData: ImageData;
  normMode: StainNormalizationMode;
  enhanceMode: StainEnhancementMode;
  normParams: NormalizationParams | null;
  priority: number; // Lower = higher priority (distance from viewport center)
  resolve: (result: ImageData) => void;
  reject: (error: Error) => void;
}

interface PendingTask {
  task: ProcessingTask;
  workerId: number;
}

/**
 * Pool of Web Workers for parallel tile processing.
 * Keeps the main thread responsive during zoom/pan.
 */
export class ProcessingWorkerPool {
  private workers: Worker[] = [];
  private pendingTasks = new Map<string, PendingTask>();
  private taskQueue: ProcessingTask[] = [];
  private workerBusy: boolean[] = [];
  private isZooming = false;
  private zoomDebounceTimer: number | null = null;
  private readonly maxConcurrentPerWorker = 2;

  constructor(private readonly poolSize: number = navigator.hardwareConcurrency || 4) {
    this.initWorkers();
  }

  private initWorkers(): void {
    for (let i = 0; i < this.poolSize; i++) {
      const worker = new ProcessingWorker();
      worker.onmessage = (event: MessageEvent<ProcessTileResponse>) => {
        this.handleWorkerResponse(i, event.data);
      };
      worker.onerror = (error) => {
        console.error(`Worker ${i} error:`, error);
        // Mark worker as not busy so it can accept new tasks
        this.workerBusy[i] = false;
        this.processNextTask();
      };
      this.workers.push(worker);
      this.workerBusy.push(false);
    }
  }

  private handleWorkerResponse(workerId: number, response: ProcessTileResponse): void {
    const pending = this.pendingTasks.get(response.id);
    if (pending) {
      this.pendingTasks.delete(response.id);
      pending.task.resolve(response.imageData);
    }
    
    // Check if worker has capacity for more tasks
    const workerTaskCount = Array.from(this.pendingTasks.values())
      .filter(p => p.workerId === workerId).length;
    
    if (workerTaskCount < this.maxConcurrentPerWorker) {
      this.workerBusy[workerId] = false;
      this.processNextTask();
    }
  }

  /**
   * Signal that a zoom gesture has started.
   * Reduces processing concurrency to prioritize rendering.
   */
  notifyZoomStart(): void {
    this.isZooming = true;
    if (this.zoomDebounceTimer !== null) {
      clearTimeout(this.zoomDebounceTimer);
    }
  }

  /**
   * Signal that a zoom gesture has ended.
   * Waits briefly before resuming full processing to avoid false positives.
   */
  notifyZoomEnd(): void {
    if (this.zoomDebounceTimer !== null) {
      clearTimeout(this.zoomDebounceTimer);
    }
    this.zoomDebounceTimer = window.setTimeout(() => {
      this.isZooming = false;
      this.zoomDebounceTimer = null;
      // Resume processing queue
      this.processNextTask();
    }, 150); // 150ms debounce after zoom ends
  }

  /**
   * Get the effective concurrency limit based on current state.
   */
  private getEffectiveConcurrency(): number {
    // During zooming, limit to fewer concurrent tasks to keep UI responsive
    return this.isZooming ? Math.max(1, Math.floor(this.poolSize / 2)) : this.poolSize;
  }

  /**
   * Submit a tile for processing.
   * Returns a promise that resolves with the processed ImageData.
   */
  process(
    id: string,
    imageData: ImageData,
    normMode: StainNormalizationMode,
    enhanceMode: StainEnhancementMode,
    normParams: NormalizationParams | null,
    priority: number = 0
  ): Promise<ImageData> {
    return new Promise((resolve, reject) => {
      const task: ProcessingTask = {
        id,
        imageData,
        normMode,
        enhanceMode,
        normParams,
        priority,
        resolve,
        reject,
      };

      // Check if already pending - if so, cancel old one
      if (this.pendingTasks.has(id)) {
        this.cancel([id]);
      }

      // Insert into queue sorted by priority
      const insertIndex = this.taskQueue.findIndex(t => t.priority > priority);
      if (insertIndex === -1) {
        this.taskQueue.push(task);
      } else {
        this.taskQueue.splice(insertIndex, 0, task);
      }

      this.processNextTask();
    });
  }

  /**
   * Process the next task in the queue if a worker is available.
   */
  private processNextTask(): void {
    if (this.taskQueue.length === 0) return;

    const effectiveConcurrency = this.getEffectiveConcurrency();
    const currentPendingCount = this.pendingTasks.size;

    // Don't exceed effective concurrency
    if (currentPendingCount >= effectiveConcurrency * this.maxConcurrentPerWorker) {
      return;
    }

    // Find an available worker (least loaded)
    let bestWorker = -1;
    let bestLoad = Infinity;

    for (let i = 0; i < this.workers.length; i++) {
      const load = Array.from(this.pendingTasks.values())
        .filter(p => p.workerId === i).length;
      if (load < this.maxConcurrentPerWorker && load < bestLoad) {
        bestLoad = load;
        bestWorker = i;
      }
    }

    if (bestWorker === -1) return;

    const task = this.taskQueue.shift();
    if (!task) return;

    // Send to worker
    const request: ProcessTileRequest = {
      type: 'process',
      id: task.id,
      imageData: task.imageData,
      normMode: task.normMode,
      enhanceMode: task.enhanceMode,
      normParams: task.normParams,
    };

    this.pendingTasks.set(task.id, { task, workerId: bestWorker });
    
    // Transfer the ArrayBuffer for zero-copy
    this.workers[bestWorker].postMessage(request, [task.imageData.data.buffer]);

    // Try to process more tasks
    this.processNextTask();
  }

  /**
   * Cancel pending or queued tasks by ID.
   */
  cancel(ids: string[]): void {
    // Remove from queue
    this.taskQueue = this.taskQueue.filter(t => !ids.includes(t.id));

    // Cancel pending tasks
    const idsToCancel: string[] = [];
    for (const id of ids) {
      const pending = this.pendingTasks.get(id);
      if (pending) {
        idsToCancel.push(id);
        pending.task.reject(new Error('Cancelled'));
        this.pendingTasks.delete(id);
      }
    }

    // Notify workers (group by worker for efficiency)
    if (idsToCancel.length > 0) {
      const cancelRequest: CancelRequest = { type: 'cancel', ids: idsToCancel };
      for (const worker of this.workers) {
        worker.postMessage(cancelRequest);
      }
    }
  }

  /**
   * Cancel all tasks not in the given set of IDs.
   * Useful when viewport changes and old tiles are no longer needed.
   */
  cancelAllExcept(keepIds: Set<string>): void {
    const toCancel: string[] = [];
    
    // Check queue
    for (const task of this.taskQueue) {
      if (!keepIds.has(task.id)) {
        toCancel.push(task.id);
      }
    }
    
    // Check pending
    for (const id of this.pendingTasks.keys()) {
      if (!keepIds.has(id)) {
        toCancel.push(id);
      }
    }

    if (toCancel.length > 0) {
      this.cancel(toCancel);
    }
  }

  /**
   * Get the number of tasks currently being processed.
   */
  get pendingCount(): number {
    return this.pendingTasks.size;
  }

  /**
   * Get the number of tasks waiting in the queue.
   */
  get queueLength(): number {
    return this.taskQueue.length;
  }

  /**
   * Terminate all workers and clean up.
   */
  destroy(): void {
    if (this.zoomDebounceTimer !== null) {
      clearTimeout(this.zoomDebounceTimer);
    }
    
    // Reject all pending tasks
    for (const pending of this.pendingTasks.values()) {
      pending.task.reject(new Error('Pool destroyed'));
    }
    this.pendingTasks.clear();
    this.taskQueue = [];

    // Terminate workers
    for (const worker of this.workers) {
      worker.terminate();
    }
    this.workers = [];
  }
}

// Singleton instance for the application
let poolInstance: ProcessingWorkerPool | null = null;

/**
 * Get the shared processing worker pool instance.
 */
export function getProcessingPool(): ProcessingWorkerPool {
  if (!poolInstance) {
    poolInstance = new ProcessingWorkerPool();
  }
  return poolInstance;
}

/**
 * Destroy the shared processing worker pool (for cleanup).
 */
export function destroyProcessingPool(): void {
  if (poolInstance) {
    poolInstance.destroy();
    poolInstance = null;
  }
}
