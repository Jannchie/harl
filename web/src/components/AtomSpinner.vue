<script setup lang="ts">
/**
 * 唯一的 loading 标记，照搬 pfh（apps/web/src/components/AtomSpinner.vue）：三条轨道绕同一个中心转，
 * 出自 loading.dev 的 atom（https://loading.dev/spinners/atom，MIT，Jakub Krehel），去掉了外壳。
 *
 * 每条轨道是宽度随转角余弦变化的椭圆，描边粗细一圈一致；椭圆预先画成一条 FRAMES 帧的长条，
 * 动画只是逐帧平移——平移交给合成线程，主线程再忙也不卡。
 *
 * 调用方像给图标定尺寸一样给它 h-4 w-4；颜色是它自己的（ink-muted），不随所在位置变。
 * pfh 里转速跟着 provider 变（SPINNER_PACE），harl 只有一种速度，这里不要。
 */
const ORBITS = [0, 1, 2]
const TILT = 180 / ORBITS.length

/** 半圈的帧数：环的宽度每半圈重复一次 */
const FRAMES = 64
const RADIUS = 11
// 每帧的宽度是转角的余弦；不取 0，没有宽度的椭圆画不出描边
const FRAME_RADII = Array.from({ length: FRAMES }, (_, frame) =>
  Math.max(0.01, RADIUS * Math.abs(Math.cos((Math.PI * frame) / FRAMES))),
)
</script>

<template>
  <span class="atom ink-muted" aria-hidden="true">
    <span v-for="index in ORBITS" :key="index" class="atom-orbit" :style="{ transform: `rotate(${index * TILT}deg)` }">
      <svg class="atom-strip" :viewBox="`0 0 ${24 * FRAMES} 24`" :style="{ '--atom-step': index, width: `${100 * FRAMES}%` }">
        <ellipse v-for="(rx, frame) in FRAME_RADII" :key="frame" :cx="24 * frame + 12" cy="12" :rx="rx" :ry="RADIUS" />
      </svg>
    </span>
  </span>
</template>

<style scoped>
.atom {
  --atom-duration: 2400ms;
  position: relative;
  display: inline-block;
  flex-shrink: 0;
}

.atom-orbit {
  position: absolute;
  inset: 0;
  overflow: hidden;
}

.atom-strip {
  display: block;
  height: 100%;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.9;
  /* 半圈一次，逐帧跳，窗口里始终只露出一个椭圆 */
  animation: atom-turn calc(var(--atom-duration) / 2) steps(64) infinite;
  animation-delay: calc(var(--atom-duration) * (var(--atom-step) - 3) / 3);
}

@keyframes atom-turn {
  to {
    transform: translateX(-100%);
  }
}

@media (prefers-reduced-motion: reduce) {
  .atom-strip {
    animation: none;
    transform: translateX(calc(-100% / 64 * 21));
  }
}
</style>
