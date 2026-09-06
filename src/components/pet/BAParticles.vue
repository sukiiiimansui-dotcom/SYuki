<template>
  <canvas ref="canvasRef" class="w-full h-full pointer-events-none"></canvas>
</template>

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from "vue";

const props = defineProps({
  particleCount: { type: Number, default: 35 },
  speed: { type: Number, default: 0.6 },
});

const canvasRef = ref<HTMLCanvasElement | null>(null);
let animationFrameId: number;
let particles: Particle[] = [];
let width = 0;
let height = 0;

const colors = ["#00BFFF", "#87CEFA", "#FFFFFF", "#E0F7FA"];
const types = ["circle", "circle", "cross"];

class Particle {
  x: number = 0;
  y: number = 0;
  size: number = 0;
  speedX: number = 0;
  speedY: number = 0;
  color: string = "";
  type: string = "";
  opacity: number = 0;
  maxOpacity: number = 0;

  constructor() {
    this.reset(true);
  }

  reset(isInit = false) {
    this.x = Math.random() * width;
    this.y = isInit ? Math.random() * height : -(Math.random() * 20 + 10);
    this.size = Math.random() * 1.5 + 0.8;
    this.speedY = (Math.random() * 0.4 + 0.3) * props.speed;
    this.speedX = (Math.random() - 0.5) * 0.15 * props.speed;
    this.color = colors[Math.floor(Math.random() * colors.length)];
    this.type = types[Math.floor(Math.random() * types.length)];
    this.opacity = isInit ? Math.random() * 0.6 : 0;
    this.maxOpacity = Math.random() * 0.5 + 0.3;
  }

  update() {
    this.x += this.speedX;
    this.y += this.speedY;

    const progress = this.y / height;

    if (progress < 0.15) {
      this.opacity = (progress / 0.15) * this.maxOpacity;
    } else if (progress > 0.8) {
      this.opacity = ((1 - progress) / 0.2) * this.maxOpacity;
    } else {
      this.opacity = this.maxOpacity;
    }

    this.opacity = Math.max(0, Math.min(this.maxOpacity, this.opacity));

    if (this.y > height + 10) {
      this.reset();
    }
  }

  draw(ctx: CanvasRenderingContext2D) {
    ctx.save();
    ctx.globalAlpha = this.opacity;
    ctx.fillStyle = this.color;
    ctx.strokeStyle = this.color;

    if (this.type === "circle") {
      ctx.beginPath();
      ctx.arc(this.x, this.y, this.size, 0, Math.PI * 2);
      ctx.fill();
    } else if (this.type === "cross") {
      const s = this.size * 1.2;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(this.x - s, this.y);
      ctx.lineTo(this.x + s, this.y);
      ctx.moveTo(this.x, this.y - s);
      ctx.lineTo(this.x, this.y + s);
      ctx.stroke();
    }

    ctx.restore();
  }
}

const resizeCanvas = () => {
  if (!canvasRef.value) return;
  const parent = canvasRef.value.parentElement;
  if (parent) {
    width = parent.clientWidth;
    height = parent.clientHeight;
    canvasRef.value.width = width;
    canvasRef.value.height = height;
  }
};

// 帧率上限 + 页面隐藏暂停：背景装饰气泡 30fps 足够，避免 60fps 常驻空转
const TARGET_FPS = 30;
const FRAME_INTERVAL = 1000 / TARGET_FPS;
let lastTime = 0;
let running = false;

const loop = () => {
  if (!running || !canvasRef.value) return;

  const now = performance.now();
  if (now - lastTime >= FRAME_INTERVAL) {
    lastTime = now;
    const ctx = canvasRef.value.getContext("2d");
    if (!ctx) return;

    ctx.clearRect(0, 0, width, height);

    particles.forEach((p) => {
      p.update();
      p.draw(ctx);
    });
  }

  animationFrameId = requestAnimationFrame(loop);
};

const startLoop = () => {
  if (running) return;
  running = true;
  lastTime = 0;
  loop();
};

const stopLoop = () => {
  running = false;
  cancelAnimationFrame(animationFrameId);
};

const handleVisibility = () => {
  if (document.hidden) stopLoop();
  else if (!running) startLoop();
};

onMounted(() => {
  resizeCanvas();
  window.addEventListener("resize", resizeCanvas);
  document.addEventListener("visibilitychange", handleVisibility);

  for (let i = 0; i < props.particleCount; i++) {
    particles.push(new Particle());
  }

  startLoop();
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", resizeCanvas);
  document.removeEventListener("visibilitychange", handleVisibility);
  stopLoop();
});
</script>
