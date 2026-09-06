import { ref, onMounted, onUnmounted, watch, nextTick, type Ref } from 'vue'
import type {
  FallingParticle,
  FallingParticleConfig,
  UseFallingParticleOptions,
  UseFallingParticleReturn,
  KeyframeConfig,
  ParticleCustomization,
} from '../types/falling'

/**
 * Generate a unique ID for particles
 */
export function generateParticleId(): string {
  return Math.random().toString(36).substr(2, 9)
}

/**
 * Calculate random value within a range
 */
export function randomInRange(min: number, max: number): number {
  return Math.random() * (max - min) + min
}

/**
 * Default particle factory - creates a basic falling particle
 */
export function createDefaultParticle(
  id: string,
  config: FallingParticleConfig,
  customization?: ParticleCustomization,
): FallingParticle {
  const size = randomInRange(config.minSize, config.maxSize)
  const left = Math.random() * window.innerWidth
  const duration = randomInRange(config.minDuration, config.maxDuration)
  const opacity = randomInRange(config.minOpacity, config.maxOpacity)
  const horizontalMovement = randomInRange(-config.horizontalRange, config.horizontalRange)

  // When randomStartY is enabled, use negative delay so particles appear
  // distributed across the screen at different Y positions instead of
  // all starting from the top
  const delay = config.randomStartY ? -Math.random() * duration : Math.random() * config.maxDelay

  return {
    id,
    size,
    left,
    top: config.initialTopOffset,
    opacity,
    duration,
    delay,
    horizontalMovement,
    ...customization,
  }
}

/**
 * 默认粒子变量生成器 - 生成每个粒子动画所需的 CSS 变量。
 *
 * 不再为每个粒子单独注入一个 <style>（50~75 个 style 元素会大量消耗 DOM 与样式
 * 重算）。改为「1 个共享 keyframes + 每粒子 CSS 变量」：动画的位移/旋转/透明度
 * 全部走 CSS 变量，粒子的随机值只在创建时算一次、随元素样式落地。
 */
export function createDefaultParticleVars(
  particle: FallingParticle,
  maxHeight: number,
  keyframeConfig: KeyframeConfig,
): Record<string, string> {
  const { rotation, opacity } = keyframeConfig
  const ranges = rotation.rotationRanges
  const kf = opacity.keyframes
  const r0 = rotation.startRotation
  const r25 = rotation.startRotation + randomInRange(ranges[0]!.min, ranges[0]!.max)
  const r50 = rotation.startRotation + randomInRange(ranges[1]!.min, ranges[1]!.max)
  const r75 = rotation.startRotation + randomInRange(ranges[2]!.min, ranges[2]!.max)
  const r100 = rotation.startRotation + randomInRange(ranges[3]!.min, ranges[3]!.max)
  return {
    '--hm': `${particle.horizontalMovement}px`,
    '--fr0': `${r0}deg`,
    '--fr25': `${r25}deg`,
    '--fr50': `${r50}deg`,
    '--fr75': `${r75}deg`,
    '--fr100': `${r100}deg`,
    '--fo0': `${particle.opacity * kf[0]!}`,
    '--fo25': `${particle.opacity * kf[1]!}`,
    '--fo50': `${particle.opacity * kf[2]!}`,
    '--fo75': `${particle.opacity * kf[3]!}`,
    '--fo100': `${particle.opacity * kf[4]!}`,
  }
}

/**
 * 共享 keyframes：雪的旋转/位移/透明度全部用 CSS 变量表达，粒子只需覆盖变量。
 * `--fh` 为容器高度（maxHeight），`--hm` 为水平位移，`--fr*`/`--fo*` 为逐帧旋转/透明度。
 */
const SHARED_KEYFRAMES = `
@keyframes fall-shared {
  0%   { transform: translate(0, 0) rotate(var(--fr0, 0deg)); opacity: var(--fo0, 1); }
  25%  { transform: translate(calc(var(--hm, 0px) * 0.25), calc(var(--fh, 100vh) * 0.25)) rotate(var(--fr25, 0deg)); opacity: var(--fo25, 1); }
  50%  { transform: translate(calc(var(--hm, 0px) * 0.5), calc(var(--fh, 100vh) * 0.5)) rotate(var(--fr50, 0deg)); opacity: var(--fo50, 1); }
  75%  { transform: translate(calc(var(--hm, 0px) * 0.75), calc(var(--fh, 100vh) * 0.75)) rotate(var(--fr75, 0deg)); opacity: var(--fo75, 1); }
  100% { transform: translate(var(--hm, 0px), var(--fh, 100vh)) rotate(var(--fr100, 0deg)); opacity: var(--fo100, 1); }
}
`
let sharedKeyframesInjected = false
function ensureSharedKeyframes() {
  if (sharedKeyframesInjected || typeof document === 'undefined') return
  sharedKeyframesInjected = true
  const style = document.createElement('style')
  style.setAttribute('data-falling-particles', 'shared-keyframes')
  style.textContent = SHARED_KEYFRAMES
  document.head.appendChild(style)
}

export function useFallingParticle<T extends FallingParticle>(
  props: { enabled?: boolean; intensity?: number },
  options: UseFallingParticleOptions<T>,
  containerRef: Ref<HTMLElement | null>,
): UseFallingParticleReturn<T> {
  const { config, baseCount, createParticle, computeVars } = options

  // Normalize props with defaults
  const enabled = props.enabled ?? true
  const intensity = props.intensity ?? 1

  // Reactive state
  const particles = ref<T[]>([]) as Ref<T[]>

  const maxHeight = ref(0)
  const particleCount = ref(Math.floor(baseCount * intensity))

  /**
   * 收集粒子动画 CSS 变量，并确保共享 keyframes 已被注入（仅一次）。
   * 不再为每个粒子追加一个 <style>。
   */
  const createParticleAnimation = (particle: T): void => {
    ensureSharedKeyframes()
    particle.cssVars = computeVars(particle, maxHeight.value) as Record<string, string>
  }

  /**
   * Create multiple particles
   */
  const createParticles = (count: number): void => {
    for (let i = 0; i < count; i++) {
      const id = generateParticleId()
      const particle = createParticle(id, config)
      particle.id = id
      createParticleAnimation(particle)
      particles.value.push(particle)
    }
  }

  /**
   * Remove all particles and cleanup stylesheets
   */
  const removeAllParticles = (): void => {
    particles.value.forEach((particle) => {
      if (particle.styleSheet && particle.styleSheet.parentNode) {
        particle.styleSheet.parentNode.removeChild(particle.styleSheet)
      }
    })
    particles.value = []
  }

  /**
   * Set max height from container or window
   */
  const setMaxHeight = (): void => {
    if (containerRef.value && containerRef.value.parentElement) {
      maxHeight.value = containerRef.value.parentElement.clientHeight
    } else {
      maxHeight.value = window.innerHeight
    }
  }

  /**
   * Recreate all particles (used for resize events)
   */
  const recreateParticles = (): void => {
    removeAllParticles()
    createParticles(particleCount.value)
  }

  // Watch for intensity changes
  watch(
    () => props.intensity,
    (newIntensity) => {
      particleCount.value = Math.floor(baseCount * (newIntensity ?? 1))
      recreateParticles()
    },
  )

  // Watch for enabled state changes
  watch(
    () => props.enabled,
    (newVal) => {
      if (newVal ?? true) {
        setMaxHeight()
        createParticles(particleCount.value)
      } else {
        removeAllParticles()
      }
    },
  )

  // Lifecycle hooks
  onMounted(() => {
    nextTick(() => {
      setMaxHeight()
      if (enabled) {
        createParticles(particleCount.value)
      }
    })

    window.addEventListener('resize', () => {
      setMaxHeight()
      recreateParticles()
    })
  })

  onUnmounted(() => {
    removeAllParticles()
    window.removeEventListener('resize', setMaxHeight)
  })

  return {
    particles,
    maxHeight,
    createParticles,
    removeAllParticles,
    recreateParticles,
    setMaxHeight,
    particleCount,
  }
}
