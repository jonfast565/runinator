<template>
  <span class="brand-mark" aria-hidden="true">
    <span class="brand-mark-pulse"></span>
    <img class="brand-mark-glyph" src="/icon.png" width="28" height="28" alt="" />
  </span>
</template>

<script setup lang="ts"></script>

<style scoped>
.brand-mark {
  position: relative;
  display: grid;
  flex: 0 0 auto;
  width: 28px;
  height: 28px;
  place-items: center;
}

.brand-mark-glyph {
  display: block;
  border-radius: 7px;
  animation: brand-run 3.2s cubic-bezier(0.22, 1, 0.36, 1) infinite;
  will-change: transform;
}

.brand-mark-pulse {
  position: absolute;
  inset: 3px;
  border: 1px solid color-mix(in srgb, var(--accent-pulse) 72%, transparent);
  border-radius: 9px;
  animation: brand-pulse 3.2s ease-out infinite;
  pointer-events: none;
}

@keyframes brand-run {
  0%,
  100% {
    transform: translateY(0) scale(1);
  }

  38% {
    transform: translateY(-1px) scale(1.035);
  }

  54% {
    transform: translateY(0) scale(1);
  }
}

@keyframes brand-pulse {
  0%,
  52%,
  100% {
    opacity: 0;
    transform: scale(0.8);
  }

  16% {
    opacity: 0.65;
    transform: scale(1);
  }

  44% {
    opacity: 0;
    transform: scale(1.45);
  }
}

@media (prefers-reduced-motion: reduce) {
  .brand-mark-glyph,
  .brand-mark-pulse {
    animation: none;
  }
}
</style>
