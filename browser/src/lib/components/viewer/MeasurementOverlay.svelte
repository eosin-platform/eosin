<script lang="ts">
  import { settings, type MeasurementUnit } from '$lib/stores/settings';
  import type { ViewportState } from '$lib/frusta/viewport';

  interface MeasurementState {
    /** Whether measurement mode is active */
    active: boolean;
    /** Measurement mode: 'drag' for middle-click drag, 'pending' for waiting first click, 'placing' for first point set, 'toggle' for second click held, 'confirmed' for sticky measurement */
    mode: 'drag' | 'pending' | 'placing' | 'toggle' | 'confirmed' | null;
    /** Start position in screen coordinates */
    startScreen: { x: number; y: number } | null;
    /** End position in screen coordinates */
    endScreen: { x: number; y: number } | null;
    /** Start position in image coordinates (level 0 pixels) */
    startImage: { x: number; y: number } | null;
    /** End position in image coordinates (level 0 pixels) */
    endImage: { x: number; y: number } | null;
  }

  interface Props {
    /** Current viewport state */
    viewport: ViewportState;
    /** Microns per pixel at full resolution (provided by slide metadata) */
    micronsPerPixel?: number;
    /** Current measurement state */
    measurement: MeasurementState;
  }

  let { viewport, micronsPerPixel = 0.25, measurement }: Props = $props();

  // Get settings for units
  let units = $derived($settings.measurements.units);

  // Convert microns to display string with unit
  function formatDistance(microns: number, unit: MeasurementUnit): string {
    switch (unit) {
      case 'um':
        if (microns >= 1000) {
          return `${(microns / 1000).toFixed(microns >= 10000 ? 0 : 1)} mm`;
        }
        return `${microns.toFixed(1)} µm`;
      case 'mm':
        return `${(microns / 1000).toFixed(microns >= 1000 ? 1 : 3)} mm`;
      case 'in':
        const inches = microns / 25400;
        if (inches >= 0.1) {
          return `${inches.toFixed(2)} in`;
        }
        return `${(inches * 1000).toFixed(1)} mil`;
      default:
        return `${microns.toFixed(1)} µm`;
    }
  }

  // Calculate distance in microns from image coordinates
  const distanceMicrons = $derived(() => {
    if (!measurement.startImage || !measurement.endImage) return 0;
    const dx = measurement.endImage.x - measurement.startImage.x;
    const dy = measurement.endImage.y - measurement.startImage.y;
    const distancePixels = Math.sqrt(dx * dx + dy * dy);
    return distancePixels * micronsPerPixel;
  });

  // Get display text
  const displayText = $derived(() => formatDistance(distanceMicrons(), units));

  // Calculate screen positions for the line (converting from image coords using current viewport)
  const screenStart = $derived(() => {
    if (!measurement.startImage) return null;
    const x = (measurement.startImage.x - viewport.x) * viewport.zoom;
    const y = (measurement.startImage.y - viewport.y) * viewport.zoom;
    return { x, y };
  });

  const screenEnd = $derived(() => {
    if (!measurement.endImage) return null;
    const x = (measurement.endImage.x - viewport.x) * viewport.zoom;
    const y = (measurement.endImage.y - viewport.y) * viewport.zoom;
    return { x, y };
  });

  // Calculate label position (midpoint of line, slightly offset)
  const labelPosition = $derived(() => {
    const start = screenStart();
    const end = screenEnd();
    if (!start || !end) return null;
    
    const midX = (start.x + end.x) / 2;
    const midY = (start.y + end.y) / 2;
    
    // Calculate perpendicular offset for label
    const dx = end.x - start.x;
    const dy = end.y - start.y;
    const length = Math.sqrt(dx * dx + dy * dy);
    
    if (length < 1) return { x: midX, y: midY - 20 };
    
    // Perpendicular unit vector
    const perpX = -dy / length;
    const perpY = dx / length;
    
    // Offset by 20 pixels in perpendicular direction (choose upward when possible)
    const offsetMagnitude = 20;
    const offsetX = perpX * offsetMagnitude;
    const offsetY = perpY * offsetMagnitude;
    
    // Choose the offset that moves the label "up" (negative Y in screen coords)
    if (offsetY > 0) {
      return { x: midX - offsetX, y: midY - offsetY };
    }
    return { x: midX + offsetX, y: midY + offsetY };
  });

  // Calculate line angle for potential rotation of label
  const lineAngle = $derived(() => {
    const start = screenStart();
    const end = screenEnd();
    if (!start || !end) return 0;
    
    const dx = end.x - start.x;
    const dy = end.y - start.y;
    return Math.atan2(dy, dx) * (180 / Math.PI);
  });
</script>

{#if measurement.active && screenStart() && screenEnd()}
  <svg class="measurement-overlay" xmlns="http://www.w3.org/2000/svg">
    <!-- Main measurement line -->
    <line
      x1={screenStart()?.x}
      y1={screenStart()?.y}
      x2={screenEnd()?.x}
      y2={screenEnd()?.y}
      class="measurement-line"
    />
    
    <!-- Start point marker -->
    <circle
      cx={screenStart()?.x}
      cy={screenStart()?.y}
      r="4"
      class="measurement-point"
    />
    
    <!-- End point marker -->
    <circle
      cx={screenEnd()?.x}
      cy={screenEnd()?.y}
      r="4"
      class="measurement-point"
    />
    
    <!-- Distance label -->
    {#if labelPosition()}
      <g transform="translate({labelPosition()?.x}, {labelPosition()?.y})">
        <!-- Label background -->
        <rect
          x="-30"
          y="-12"
          width="60"
          height="20"
          rx="4"
          class="label-background"
        />
        <!-- Label text -->
        <text
          x="0"
          y="4"
          text-anchor="middle"
          class="label-text"
        >
          {displayText()}
        </text>
      </g>
    {/if}
  </svg>
{/if}

<style>
  .measurement-overlay {
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

  .measurement-line {
    stroke: var(--secondary-hex);
    stroke-width: 2;
    stroke-linecap: round;
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.5));
  }

  .measurement-point {
    fill: var(--secondary-hex);
    stroke: white;
    stroke-width: 2;
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.5));
  }

  .label-background {
    fill: rgba(0, 0, 0, 0.75);
  }

  .label-text {
    fill: white;
    font-size: 12px;
    font-weight: 600;
    font-family: system-ui, -apple-system, sans-serif;
  }
</style>
