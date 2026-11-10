import type zh from '../zh-CN/ctxCurrent'

export default {
  windowCatalog: '{n} window (catalog)',
  windowCatalogHint: 'The log does not record the context window; using the models.dev catalog window for {model}',
  free: 'Free window',
  trigger: 'Auto-compaction trigger',
  triggerTip: 'Compaction fires when usage reaches this line',
  triggerAt: 'compacts at {pct}',
  triggerAtHint: 'Auto-compaction threshold {n} tokens: min(80% of window, window − output reserve − 64k)',
  headroomTip: 'Auto-compaction is expected to fire at {pct} of the window',
  headroomBuffer: 'Buffer kept free ahead of the output reserve',
  outputTip: "The model's maximum output tokens per request",
} satisfies typeof zh
