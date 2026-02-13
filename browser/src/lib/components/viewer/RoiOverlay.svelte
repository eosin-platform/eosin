<script lang="ts">
  import type { ViewportState } from '$lib/frusta/viewport';

  interface RoiState {
    /** Whether ROI mode is active */
    active: boolean;
    /** ROI mode: 'pending' for waiting first click, 'placing' for first corner set, 'toggle' for second click held, 'confirmed' for sticky ROI */
    mode: 'pending' | 'placing' | 'toggle' | 'confirmed' | null;
    /** Start position in image coordinates (level 0 pixels) - first corner */
    startImage: { x: number; y: number } | null;
    /** End position in image coordinates (level 0 pixels) - second corner */
    endImage: { x: number; y: number } | null;
  }

  interface Props {
    /** Current viewport state */
    viewport: ViewportState;
    /** Current ROI state */
    roi: RoiState;
  }

  let { viewport, roi }: Props = $props();

  // Calculate screen positions for the rectangle corners
  const screenStart = $derived(() => {
    if (!roi.startImage) return null;
    const x = (roi.startImage.x - viewport.x) * viewport.zoom;
    const y = (roi.startImage.y - viewport.y) * viewport.zoom;
    return { x, y };
  });

  const screenEnd = $derived(() => {
    if (!roi.endImage) return null;
    const x = (roi.endImage.x - viewport.x) * viewport.zoom;
    const y = (roi.endImage.y - viewport.y) * viewport.zoom;
    return { x, y };
  });

  // Calculate rectangle bounds (top-left and dimensions)
  const rectBounds = $derived(() => {
    const start = screenStart();
    const end = screenEnd();
    if (!start || !end) return null;

    const x = Math.min(start.x, end.x);
    const y = Math.min(start.y, end.y);
    const width = Math.abs(end.x - start.x);
    const height = Math.abs(end.y - start.y);

    return { x, y, width, height };
  });
</script>

{#if roi.active && rectBounds()}
  <svg class="roi-overlay" xmlns="http://www.w3.org/2000/svg">
    <!-- ROI rectangle with animated dashed border -->
    <rect
      x={rectBounds()?.x}
      y={rectBounds()?.y}
      width={rectBounds()?.width}
      height={rectBounds()?.height}
      class="roi-rect"
    />
  </svg>
{/if}

<style>
  .roi-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 25;
    overflow: visible;
    /* Prevent selection on touch devices */
    -webkit-touch-callout: none;
    -webkit-user-select: none;
    user-select: none;
  }

  .roi-rect {
    fill: none;
    stroke: #fbbf24;
    stroke-width: 2;
    stroke-dasharray: 8 4;
    stroke-linecap: round;
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.5));
    animation: dash-march 0.5s linear infinite;
  }

  @keyframes dash-march {
    to {
      stroke-dashoffset: -12;
    }
  }
</style>
