<template>
  <svg
    class="brand-mark-glyph"
    width="26"
    height="21"
    viewBox="0 0 30 24"
    fill="none"
    aria-hidden="true"
    focusable="false"
  >
    <!-- a waiting node reaching, along an edge, toward the one currently live -->
    <circle
      cx="6"
      cy="17"
      r="2.6"
      stroke-width="1.4"
      style="fill: var(--surface-inverse); stroke: var(--text-inverse-faint)"
    />
    <path
      d="M8.4 15.2 C 13 12.4, 17.5 9.2, 21.6 6.6"
      stroke-width="1.5"
      stroke-linecap="round"
      fill="none"
      style="stroke: var(--accent)"
    />
    <circle
      class="brand-mark-pulse"
      cx="23"
      cy="5.6"
      r="6"
      opacity="0.28"
      style="fill: var(--accent-pulse)"
    />
    <circle cx="23" cy="5.6" r="2.9" style="fill: var(--accent-pulse)" />
  </svg>
</template>

<script setup lang="ts"></script>

<style scoped>
.brand-mark-glyph {
  display: block;
  flex: 0 0 auto;
  transition: transform 180ms cubic-bezier(0.22, 1, 0.36, 1);
}

.brand-mark-pulse {
  transform-box: fill-box;
  transform-origin: center;
  animation: brand-pulse 2.4s ease-in-out infinite;
}

@keyframes brand-pulse {
  0%,
  100% {
    opacity: 0.28;
    transform: scale(0.85);
  }

  50% {
    opacity: 0;
    transform: scale(1.7);
  }
}

@media (prefers-reduced-motion: reduce) {
  .brand-mark-pulse {
    animation: none;
    opacity: 0.16;
  }
}
</style>
