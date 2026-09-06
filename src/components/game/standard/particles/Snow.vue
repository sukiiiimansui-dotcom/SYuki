<template>
  <div class="snow-container" ref="containerRef" :style="{ '--fh': maxHeight + 'px' }">
    <div
      class="snowflake"
      v-for="(snowflake, index) in snowflakes"
      :key="index"
      :style="{
        fontSize: `${snowflake.size}px`,
        left: `${snowflake.left}px`,
        top: `${snowflake.top}px`,
        opacity: snowflake.opacity,
        ...(snowflake.cssVars as Record<string, string>),
        animation: `fall-shared ${snowflake.duration}s linear ${snowflake.delay}s infinite`,
      }"
    >
      {{ snowflake.content }}
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  useFallingParticle,
  createDefaultParticle,
  createDefaultParticleVars,
} from './hooks/useFallingParticle'
import type { FallingParticle } from './types/falling'
import { snow } from './config/snow'
import { ref } from 'vue'

const { config, settings, keyframes } = snow

// 组件属性定义
interface Props {
  enabled?: boolean
  intensity?: number
}

const containerRef = ref<HTMLElement | null>(null)

// 默认属性值
const props = withDefaults(defineProps<Props>(), {
  enabled: true,
  intensity: 1,
})

const createSnowflake = (id: string): FallingParticle => {
  const content = settings.chars[Math.floor(Math.random() * settings.chars.length)] || '❄'
  return createDefaultParticle(id, config, { content })
}

const generateSnowflakeVars = (snowflake: FallingParticle, maxHeight: number): Record<string, string> => {
  return createDefaultParticleVars(snowflake, maxHeight, keyframes)
}

const { particles: snowflakes, maxHeight } = useFallingParticle<FallingParticle>(
  props,
  {
    config,
    baseCount: settings.baseCount,
    createParticle: createSnowflake,
    computeVars: generateSnowflakeVars,
  },
  containerRef,
)
</script>

<style scoped>
.snow-container {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  z-index: 9999;
  overflow: hidden;
}

.snowflake {
  position: absolute;
  color: white;
  text-align: center;
  user-select: none;
  pointer-events: none;
  text-shadow: 0 0 5px rgba(255, 255, 255, 0.5);
  opacity: 0.7;
  transform-origin: center;
}
</style>
