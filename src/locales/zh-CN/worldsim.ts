// 「世界模拟」词条（namespace: worldsim）
//
// 为什么新开一个命名空间、而不是往 views.ts 里塞：
//   ① 这一整套功能（P1~P5）会有几十条文案，混进 views.ts 会让那个文件越来越难维护；
//   ② 官方规矩是**只写 zh-CN 就合规**（i18n 的 fallbackLocale 是 zh-CN，
//      en/ja/zh-HK 缺键时运行时自动回落中文），所以这里先只落中文。
//
// 注意：应用启动时会把内置词条**播种**到数据目录（见 locales/index.ts 的 BUNDLE_VERSION），
// 所以这里改了文案要重新播种才生效 —— 词条版本号是由全部内置词条算出来的哈希，
// 新增命名空间会让版本变化，下次启动会自动重新播种，不用手动清缓存。
export default {
  /** 主菜单入口：主菜单 →「开始游戏」那一排的第四个 */
  entry: '世界模拟',
  title: '世界模拟',
  /** 主按钮 */
  enter: '进入',
  generate: '生成小区地图',
  reselect: '换个区县',
  reLocate: '重新定位',
  manual: '手动选城市',
  ipEstimate: '用 IP 估测',
  restart: '重新引导',
  back: '返回',
  /** 阶段标签 */
  stage: {
    boot: '初始化',
    locating: '定位中',
    locateFailed: '待选位置',
    manual: '手动选择',
    confirm: '确认位置',
    neighborhood: '小区',
  },
}
