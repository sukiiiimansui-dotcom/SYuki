<template>
  <!--
    加载动画（纯 CSS/SVG，不引第三方库）
    机主的要求是「只要有 5 秒以上等待时长的地方都做」，所以四个真正会等的场景各有一套：
      init   初始化（展开世界）—— 引导首屏
      locate 定位中            —— GPS/IP 可能要 4~6 秒
      map    地图加载/区域切换  —— 每次下钻都要等后端渲染一张新图
      draw   AI 绘制中          —— 大模型逐栋吐元素，几十秒起步
    四套共用同一套色板（--ws-* 变量），所以换主题/深色不用改这里一行。
  -->
  <div class="ws-loading" :class="[`ws-loading--${size}`, { 'ws-loading--inline': inline, 'ws-loading--enter': variant === 'init' }]">
    <svg class="ws-loading__art" viewBox="0 0 100 100" role="img" :aria-label="text || '加载中'">
      <!-- ① 初始化：一张正在展开的世界（外圈经线 + 内圈纬线反向转） -->
      <template v-if="variant === 'init'">
        <circle cx="50" cy="50" r="30" fill="none" stroke="var(--ws-primary)" stroke-width="2.4" opacity="0.55" />
        <ellipse cx="50" cy="50" rx="30" ry="12" fill="none" stroke="var(--ws-primary)" stroke-width="2" opacity="0.75" class="ws-anim-spin" />
        <ellipse cx="50" cy="50" rx="12" ry="30" fill="none" stroke="var(--ws-accent)" stroke-width="2" opacity="0.75" class="ws-anim-spin-rev" />
        <circle cx="50" cy="50" r="4.6" fill="var(--ws-primary-deep)" />
        <path d="M20 84 Q50 70 80 84" fill="none" stroke="var(--ws-accent)" stroke-width="2.2" stroke-linecap="round" opacity="0.7" />
      </template>

      <!-- ② 定位：雷达脉冲 + 定位针 -->
      <template v-else-if="variant === 'locate'">
        <circle class="ws-anim-radar" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-primary)" stroke-width="2" />
        <circle class="ws-anim-radar" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-accent)" stroke-width="2" style="animation-delay: 0.7s" />
        <circle class="ws-anim-radar" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-primary)" stroke-width="2" style="animation-delay: 1.4s" />
        <circle cx="50" cy="50" r="5" fill="var(--ws-primary)" />
        <path
          d="M50 26 c-7.5 0-13 5.6-13 13 0 9.5 13 25 13 25 s13-15.5 13-25 c0-7.4-5.5-13-13-13z"
          fill="var(--ws-primary-deep)"
          opacity="0.92"
        />
        <circle cx="50" cy="39.5" r="4.6" fill="var(--ws-bg)" />
      </template>

      <!-- ③ 地图加载 / 区域切换：等高线一圈圈扩开 -->
      <template v-else-if="variant === 'map'">
        <path d="M18 30 L38 22 L62 30 L82 22 L82 70 L62 78 L38 70 L18 78 Z" fill="none" stroke="var(--ws-accent)" stroke-width="2.2" stroke-linejoin="round" opacity="0.7" />
        <path d="M38 22 L38 70 M62 30 L62 78" fill="none" stroke="var(--ws-accent)" stroke-width="1.6" opacity="0.45" />
        <circle class="ws-anim-ring" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-primary)" stroke-width="2.4" />
        <circle class="ws-anim-ring" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-primary)" stroke-width="2.4" style="animation-delay: 0.8s" />
        <circle class="ws-anim-ring" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-primary)" stroke-width="2.4" style="animation-delay: 1.6s" />
      </template>

      <!-- ④ AI 绘制：一支笔把虚线描成实线 -->
      <template v-else>
        <path
          d="M22 74 C34 40 48 88 60 46 C66 27 74 34 80 28"
          fill="none"
          stroke="var(--ws-primary)"
          stroke-width="2.6"
          stroke-linecap="round"
          stroke-dasharray="120"
          class="ws-anim-draw"
        />
        <path d="M22 74 L46 84 L78 82" fill="none" stroke="var(--ws-accent)" stroke-width="1.8" opacity="0.5" stroke-dasharray="4 5" />
        <circle cx="80" cy="28" r="4.2" fill="var(--ws-primary-deep)" class="ws-anim-float" />
      </template>
    </svg>

    <div v-if="text || sub" class="ws-loading__body">
      <!-- 文案自带省略号时不再叠动画点（否则会出现「… .」两个点） -->
      <div v-if="text" class="ws-loading__text">
        <span :class="{ 'ws-dots': !/…$/.test(text) }">{{ text }}</span>
      </div>
      <div v-if="sub" class="ws-loading__sub">{{ sub }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
// 四个变体各自画一套小插画：不依赖任何图片资源（APK 里少一张图就少一份打包风险）
withDefaults(
  defineProps<{
    /** 场景：初始化 / 定位 / 地图 / AI 绘制 */
    variant?: 'init' | 'locate' | 'map' | 'draw'
    text?: string
    sub?: string
    /** 横排（放在进度卡、按钮旁边那种小尺寸） */
    inline?: boolean
    size?: 'sm' | 'md'
  }>(),
  { variant: 'map', text: '', sub: '', inline: false, size: 'md' },
)
</script>

<style scoped>
/* 动画与配色都在 worldsim.css（跨组件复用），这里只管排版 */
.ws-loading__body {
  display: flex;
  flex-direction: column;
  gap: 0.24em;
}
.ws-loading--inline .ws-loading__body {
  align-items: flex-start;
}
</style>
