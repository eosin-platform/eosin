<script lang="ts">
  import { settings, type MeasurementUnit } from '$lib/stores/settings';
  import type { ViewportState } from '$lib/frusta/viewport';

  interface Props {
    /** Current viewport state */
    viewport: ViewportState;
    /** Microns per pixel at full resolution (provided by slide metadata) */
    micronsPerPixel?: number;
  }

  let { viewport, micronsPerPixel = 0.25 }: Props = $props();

  // Get settings
  let units = $derived($settings.measurements.units);
  let visible = $derived($settings.image.scaleBarVisible);

  // Calculate the visible area width in microns at current zoom
  // viewport.zoom is pixels-per-image-pixel, so visible width in image pixels is viewport.width / zoom
  const visibleWidthMicrons = $derived(() => {
    const visibleImagePixels = viewport.width / Math.max(viewport.zoom, 1e-6);
    return visibleImagePixels * micronsPerPixel;
  });

  // Choose a nice round number for the scale bar
  const niceNumbers = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000, 50000, 100000];
  
  // Target: scale bar should be roughly 10-20% of viewport width
  const targetWidthMicrons = $derived(() => visibleWidthMicrons() * 0.15);

  const scaleBarMicrons = $derived(() => {
    const target = targetWidthMicrons();
    // Find the largest nice number that's <= target
    for (let i = niceNumbers.length - 1; i >= 0; i--) {
      if (niceNumbers[i] <= target) {
        return niceNumbers[i];
      }
    }
    return niceNumbers[0];
  });

  // Scale bar width in screen pixels
  const scaleBarWidth = $derived(() => {
    const imagePixels = scaleBarMicrons() / micronsPerPixel;
    return imagePixels * viewport.zoom;
  });

  // Convert microns to display string with unit
  function formatDistance(microns: number, unit: MeasurementUnit): string {
    switch (unit) {
      case 'um':
        if (microns >= 1000) {
          return `${(microns / 1000).toFixed(microns >= 10000 ? 0 : 1)} mm`;
        }
        return `${microns} µm`;
      case 'mm':
        return `${(microns / 1000).toFixed(microns >= 1000 ? 1 : 3)} mm`;
      case 'in':
        const inches = microns / 25400;
        if (inches >= 0.1) {
          return `${inches.toFixed(2)} in`;
        }
        return `${(inches * 1000).toFixed(1)} mil`;
      default:
        return `${microns} µm`;
    }
  }

  const displayText = $derived(() => formatDistance(scaleBarMicrons(), units));
</script>

{#if visible}
  <div class="scale-bar" style="--bar-width: {scaleBarWidth()}px">
    <div class="bar"></div>
    <span class="label">{displayText()}</span>
  </div>
{/if}

<style>
  .scale-bar {
    position: absolute;
    bottom: 1rem;
    left: 1rem;
    z-index: 20;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.25rem;
    pointer-events: none;
  }

  .bar {
    width: var(--bar-width);
    min-width: 40px;
    max-width: 200px;
    height: 4px;
    background: white;
    border-radius: 2px;
    box-shadow: 
      0 1px 2px rgba(0, 0, 0, 0.5),
      0 0 0 1px rgba(0, 0, 0, 0.3);
  }

  .label {
    font-size: 0.6875rem;
    font-weight: 600;
    color: white;
    text-shadow: 
      0 1px 2px rgba(0, 0, 0, 0.8),
      0 0 4px rgba(0, 0, 0, 0.5);
    padding-left: 2px;
  }

  /* Dark theme adjustments - when on light background */
  :global(.viewer-container.light-bg) .bar {
    background: #1f2937;
    box-shadow: 
      0 1px 2px rgba(255, 255, 255, 0.3),
      0 0 0 1px rgba(255, 255, 255, 0.2);
  }

  :global(.viewer-container.light-bg) .label {
    color: #1f2937;
    text-shadow: 
      0 1px 2px rgba(255, 255, 255, 0.8),
      0 0 4px rgba(255, 255, 255, 0.5);
  }
</style>
