<template>
  <!--
    一次性确认：「你现在在 广州市·越秀区 吗？」
    机主定的规矩：**只问一次**。所以这个弹窗只由 useWorldSim 在首次引导里弹（confirmedOnce），
    用户点完「是/否」就再也不会出现；即使它挡住了地图也允许点空白处 = 否（等于去纠偏）。
  -->
  <div class="ws-confirm" @click.self="emit('no')">
    <div class="ws-confirm__box ws-card">
      <div class="ws-confirm__emoji" aria-hidden="true">📍</div>
      <div class="ws-confirm__q">
        你现在在
        <span class="ws-confirm__area">{{ area || '这里' }}</span>
        吗？
      </div>
      <div class="ws-confirm__meta">
        <span v-if="sourceLabel" class="ws-tag">来源：{{ sourceLabel }}</span>
        <span v-if="note" class="ws-confirm__note">{{ note }}</span>
      </div>
      <div class="ws-confirm__ops">
        <button class="ws-btn ws-btn--primary ws-confirm__btn" type="button" @click="emit('yes')">
          是，就在这里
        </button>
        <button class="ws-btn ws-confirm__btn" type="button" @click="emit('no')">
          不是，我来选
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
withDefaults(
  defineProps<{
    /** 已知的最深一级位置名（「广州市·越秀区」；直辖市会是「北京市·朝阳区」） */
    area?: string
    /** 定位来源（系统定位 / IP 估测（城市级））——如实告诉用户精度，别让人以为是定位不准 */
    sourceLabel?: string
    note?: string
  }>(),
  { area: '', sourceLabel: '', note: '' },
)

const emit = defineEmits<{ (e: 'yes'): void; (e: 'no'): void }>()
</script>

<style scoped>
.ws-confirm {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 1em;
  background: rgba(20, 30, 32, 0.22);
  backdrop-filter: blur(2px);
  -webkit-backdrop-filter: blur(2px);
  z-index: 5;
}
.ws-confirm__box {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.7em;
  width: min(24em, 92vw);
  padding: 1.3em 1.2em 1.1em;
  text-align: center;
  animation: ws-fade-up 0.26s ease both;
}
.ws-confirm__emoji {
  font-size: 1.9em;
  animation: ws-float 3.2s ease-in-out infinite;
}
.ws-confirm__q {
  font-size: 1.06em;
  line-height: 1.7;
}
.ws-confirm__area {
  display: inline-block;
  margin: 0 0.15em;
  padding: 0.05em 0.5em;
  font-weight: 700;
  color: var(--ws-on-primary);
  background: var(--ws-primary);
  border-radius: 0.6em;
}
.ws-confirm__meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: 0.4em;
  font-size: 0.84em;
  color: var(--ws-fg-dim);
}
.ws-confirm__note {
  text-align: center;
}
.ws-confirm__ops {
  display: flex;
  gap: 0.6em;
  width: 100%;
  margin-top: 0.2em;
}
.ws-confirm__btn {
  flex: 1;
  justify-content: center;
  padding: 0.5em 0.4em;
}
</style>
