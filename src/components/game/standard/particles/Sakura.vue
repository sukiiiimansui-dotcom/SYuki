<template>
  <div class="petal-container" ref="containerRef" :style="{ '--fh': maxHeight + 'px' }">
    <div
      class="petal"
      v-for="(petal, index) in petals"
      :key="index"
      :style="{
        width: `${petal.size}px`,
        height: `${petal.size}px`,
        left: `${petal.left}px`,
        top: `${petal.top}px`,
        opacity: petal.opacity,
        background: `linear-gradient(135deg, hsl(${petal.hue}, 100%, 85%), hsl(${petal.hue}, 100%, 75%))`,
        ...(petal.cssVars as Record<string, string>),
        animation: `fall-shared ${petal.duration}s linear ${petal.delay}s infinite`,
      }"
    ></div>
  </div>
</template>

<script setup lang="ts">
import {
  useFallingParticle,
  createDefaultParticle,
  createDefaultParticleVars,
  randomInRange,
} from './hooks/useFallingParticle'
import type { FallingParticle } from './types/falling'
import { sakura } from './config/sakura'
import { ref } from 'vue'

const { config, settings, keyframes } = sakura

interface Props {
  enabled?: boolean
  intensity?: number
}

const containerRef = ref<HTMLElement | null>(null)

const props = withDefaults(defineProps<Props>(), {
  enabled: true,
  intensity: 1,
})

const createPetal = (id: string): FallingParticle => {
  const hue = randomInRange(settings.hueMin, settings.hueMax)
  return createDefaultParticle(id, config, { hue })
}

const generatePetalVars = (petal: FallingParticle, maxHeight: number): Record<string, string> => {
  return createDefaultParticleVars(petal, maxHeight, keyframes)
}

const { particles: petals, maxHeight } = useFallingParticle<FallingParticle>(
  props,
  {
    config,
    baseCount: settings.baseCount,
    createParticle: createPetal,
    computeVars: generatePetalVars,
  },
  containerRef,
)
</script>

<style scoped>
.petal-container {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  z-index: -1;
  overflow: hidden;
}

.petal {
  position: absolute;
  border-radius: 50% 0 50% 50%;
  opacity: 0.7;
  filter: drop-shadow(0 0 5px rgba(255, 182, 193, 0.5));
  transform-origin: center;
}
</style>
