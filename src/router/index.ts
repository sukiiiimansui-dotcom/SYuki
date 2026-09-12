import { createRouter, createWebHistory } from 'vue-router'

// 导入你的组件
// 为了性能，这里我们使用路由懒加载 (lazy-loading)
// 这意味着 Credits.vue 组件只会在用户访问 /credit 路径时才会被加载
const Credits = () => import('../components/views/Credits.vue')
const ComapionMode = () => import('../components/views/CompanionMode.vue')
const MainMenu = () => import('../components/views/MainMenu.vue')
const PetMode = () => import('../components/views/PetMode.vue')
const Second = () => import('../components/views/Second.vue')
const LogWindow = () => import('../components/views/LogWindow.vue')
// 剧本编辑器体量较大，必须懒加载 —— 项目没有配 manualChunks，
// 非懒加载的 view 会整个进主 chunk
const ScriptEditor = () => import('../components/views/ScriptEditor.vue')
const Bilibili = () => import('../components/views/Bilibili.vue')
const NetMusic = () => import('../components/views/NetMusic.vue')
const MemoryPanel = () => import('../components/views/MemoryPanel.vue')
const WorldMap = () => import('../components/views/WorldMap.vue')
// 世界地图扩展页（T6-6）：四个新页面同样走懒加载，不拖慢主 chunk
const WorldDistrictLive = () => import('../components/views/worldmap/DistrictLive.vue')
const WorldDistrictViz = () => import('../components/views/worldmap/DistrictViz.vue')
const WorldMapLibrary = () => import('../components/views/worldmap/MapLibrary.vue')
const WorldPhoneOverlay = () => import('../components/views/worldmap/PhoneOverlay.vue')

// 1. 定义路由表
const routes = [
  {
    path: '/',
    name: 'MainMenu',
    component: MainMenu,
  },
  {
    path: '/world',
    name: 'WorldMap',
    component: WorldMap,
  },
  {
    path: '/chat',
    name: 'LingChat',
    component: ComapionMode,
  },
  {
    path: '/credit',
    name: 'Credits',
    component: Credits,
  },
  {
    path: '/pet',
    name: 'PetMode',
    component: PetMode,
  },
  {
    path: '/second',
    name: 'Second',
    component: Second,
  },
  {
    path: '/log-window',
    name: 'LogWindow',
    component: LogWindow,
  },
  {
    path: '/script-editor',
    name: 'ScriptEditor',
    component: ScriptEditor,
  },
  {
    path: '/bilibili',
    name: 'Bilibili',
    component: Bilibili,
  },
  {
    path: '/netmusic',
    name: 'NetMusic',
    component: NetMusic,
  },
  {
    path: '/memory',
    name: 'MemoryPanel',
    component: MemoryPanel,
  },
  // 世界地图扩展页（T6-6）：只追加，不动上面任何既有路由。
  // 用 /world/xxx 前缀挂在既有 /world 下面，语义上是一组页面。
  {
    path: '/world/district-live',
    name: 'WorldDistrictLive',
    component: WorldDistrictLive,
  },
  {
    path: '/world/district-viz',
    name: 'WorldDistrictViz',
    component: WorldDistrictViz,
  },
  {
    path: '/world/maplib',
    name: 'WorldMapLibrary',
    component: WorldMapLibrary,
  },
  {
    path: '/world/phone-overlay',
    name: 'WorldPhoneOverlay',
    component: WorldPhoneOverlay,
  },
]

// 2. 创建路由实例
const router = createRouter({
  // 使用 HTML5 History 模式，URL会更美观（例如：http://localhost:5173/credit）
  // 而不是 hash 模式 (http://localhost:5173/#/credit)
  history: createWebHistory(),
  routes, // `routes: routes` 的缩写
})

// 3. 导出路由实例
export default router
