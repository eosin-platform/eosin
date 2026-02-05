<script lang="ts">
  import type { ViewportState } from '$lib/frusta/viewport';
  import type { ImageDesc } from '$lib/frusta/protocol';

  interface Props {
    /** Image dimensions and metadata */
    image: ImageDesc;
    /** Current viewport state */
    viewport: ViewportState;
    /** Callback when viewport position changes via drag */
    onViewportChange?: (viewport: ViewportState) => void;
    /** Minimap size in pixels */
    size?: number;
  }

  let { image, viewport, onViewportChange, size = 200 }: Props = $props();

  let isDragging = $state(false);
  let minimapElement: HTMLDivElement;

  // Calculate the scale to fit the image in the minimap
  const scale = $derived(() => {
    const scaleX = size / image.width;
    const scaleY = size / image.height;
    return Math.min(scaleX, scaleY);
  });

  // Minimap dimensions (maintaining aspect ratio)
  const minimapWidth = $derived(image.width * scale());
  const minimapHeight = $derived(image.height * scale());

  // Calculate the viewport rectangle position and size in minimap coordinates
  const viewportRect = $derived(() => {
    const s = scale();
    const zoom = Math.max(viewport.zoom, 1e-6);
    
    // Visible area in image pixels
    const visibleWidth = viewport.width / zoom;
    const visibleHeight = viewport.height / zoom;
    
    return {
      x: viewport.x * s,
      y: viewport.y * s,
      width: visibleWidth * s,
      height: visibleHeight * s,
    };
  });

  function handleMouseDown(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    isDragging = true;
    updateViewportFromMouse(e);
    
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
  }

  function handleMouseMove(e: MouseEvent) {
    if (!isDragging) return;
    updateViewportFromMouse(e);
  }

  function handleMouseUp() {
    isDragging = false;
    window.removeEventListener('mousemove', handleMouseMove);
    window.removeEventListener('mouseup', handleMouseUp);
  }

  function updateViewportFromMouse(e: MouseEvent) {
    if (!minimapElement || !onViewportChange) return;

    const rect = minimapElement.getBoundingClientRect();
    const s = scale();
    const zoom = Math.max(viewport.zoom, 1e-6);
    
    // Visible area in image pixels
    const visibleWidth = viewport.width / zoom;
    const visibleHeight = viewport.height / zoom;
    
    // Mouse position relative to minimap
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;
    
    // Convert to image coordinates, centering the viewport on click point
    let newX = (mouseX / s) - (visibleWidth / 2);
    let newY = (mouseY / s) - (visibleHeight / 2);
    
    // Clamp to image bounds
    const maxX = Math.max(0, image.width - visibleWidth);
    const maxY = Math.max(0, image.height - visibleHeight);
    newX = Math.max(0, Math.min(newX, maxX));
    newY = Math.max(0, Math.min(newY, maxY));
    
    onViewportChange({
      ...viewport,
      x: newX,
      y: newY,
    });
  }

  // Touch support
  function handleTouchStart(e: TouchEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.touches.length === 1) {
      isDragging = true;
      updateViewportFromTouch(e.touches[0]);
    }
  }

  function handleTouchMove(e: TouchEvent) {
    if (!isDragging || e.touches.length !== 1) return;
    e.preventDefault();
    updateViewportFromTouch(e.touches[0]);
  }

  function handleTouchEnd() {
    isDragging = false;
  }

  function updateViewportFromTouch(touch: Touch) {
    if (!minimapElement || !onViewportChange) return;

    const rect = minimapElement.getBoundingClientRect();
    const s = scale();
    const zoom = Math.max(viewport.zoom, 1e-6);
    
    const visibleWidth = viewport.width / zoom;
    const visibleHeight = viewport.height / zoom;
    
    const touchX = touch.clientX - rect.left;
    const touchY = touch.clientY - rect.top;
    
    let newX = (touchX / s) - (visibleWidth / 2);
    let newY = (touchY / s) - (visibleHeight / 2);
    
    const maxX = Math.max(0, image.width - visibleWidth);
    const maxY = Math.max(0, image.height - visibleHeight);
    newX = Math.max(0, Math.min(newX, maxX));
    newY = Math.max(0, Math.min(newY, maxY));
    
    onViewportChange({
      ...viewport,
      x: newX,
      y: newY,
    });
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="minimap"
  bind:this={minimapElement}
  style="width: {minimapWidth}px; height: {minimapHeight}px;"
  onmousedown={handleMouseDown}
  ontouchstart={handleTouchStart}
  ontouchmove={handleTouchMove}
  ontouchend={handleTouchEnd}
>
  <!-- Background representing the full image -->
  <div class="image-area"></div>
  
  <!-- Viewport rectangle -->
  <div
    class="viewport-rect"
    class:dragging={isDragging}
    style="
      left: {viewportRect().x}px;
      top: {viewportRect().y}px;
      width: {Math.max(4, viewportRect().width)}px;
      height: {Math.max(4, viewportRect().height)}px;
    "
  ></div>
</div>

<style>
  .minimap {
    position: absolute;
    bottom: 16px;
    right: 16px;
    background: rgba(30, 30, 30, 0.9);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 4px;
    cursor: pointer;
    overflow: hidden;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
    z-index: 100;
    touch-action: none;
  }

  .image-area {
    position: absolute;
    inset: 0;
    background: rgba(80, 80, 80, 0.5);
  }

  .viewport-rect {
    position: absolute;
    border: 2px solid #3b82f6;
    background: rgba(59, 130, 246, 0.2);
    box-sizing: border-box;
    pointer-events: none;
    transition: background 0.1s ease;
  }

  .viewport-rect.dragging {
    background: rgba(59, 130, 246, 0.35);
    border-color: #60a5fa;
  }

  .minimap:hover .viewport-rect {
    border-color: #60a5fa;
  }
</style>
