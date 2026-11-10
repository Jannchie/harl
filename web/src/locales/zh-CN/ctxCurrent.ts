// ctxCurrent：当前上下文卡新增的文案（窗口来源、空闲窗口、自动压缩触发线）；原有的在 ctx.current
export default {
  /** 窗口来自 models.dev 目录（日志没记） */
  windowCatalog: '窗口 {n}（目录）',
  windowCatalogHint: '日志没记上下文窗口，按 models.dev 目录里 {model} 的窗口算',
  free: '空闲窗口',
  trigger: '自动压缩触发线',
  triggerTip: '用量到这条线就会触发压缩',
  triggerAt: '{pct} 时压缩',
  triggerAtHint: '自动压缩阈值 {n} tokens：min(80% 窗口, 窗口 − 输出预留 − 64k)',
  headroomTip: '预计在窗口的 {pct} 处自动压缩',
  headroomBuffer: '输出预留之前留空的缓冲',
  outputTip: '模型单次最大输出 tokens',
}
