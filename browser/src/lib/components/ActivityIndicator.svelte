<script lang="ts">
  /**
   * A small green circle that plays a single fade-out blink animation
   * each time `trigger` changes, signifying activity for an entity.
   */

  interface Props {
    /** Change this value to trigger a new blink animation */
    trigger: number;
  }

  let { trigger }: Props = $props();

  let blinkKey = $state(0);

  $effect(() => {
    // Access trigger to subscribe to it
    void trigger;
    // Increment key to force remount of the animation element
    blinkKey++;
  });
</script>

{#key blinkKey}
  <span class="activity-dot"></span>
{/key}

<style>
  .activity-dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background-color: #22c55e;
    opacity: 0;
    animation: blink-fade 0.6s ease-out forwards;
    flex-shrink: 0;
  }

  @keyframes blink-fade {
    0% {
      opacity: 1;
    }
    100% {
      opacity: 0;
    }
  }
</style>
