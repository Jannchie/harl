import type zh from '../zh-CN/ctxCurrent'

export default {
  windowCatalog: 'ウィンドウ {n}（カタログ）',
  windowCatalogHint: 'ログにコンテキストウィンドウの記録がないため、models.dev カタログの {model} のウィンドウを使用',
  free: '空きウィンドウ',
  trigger: '自動圧縮の発動ライン',
  triggerTip: '使用量がこのラインに達すると圧縮が発動',
  triggerAt: '{pct} で圧縮',
  triggerAtHint: '自動圧縮のしきい値 {n} tokens：min(ウィンドウの 80%, ウィンドウ − 出力予約 − 64k)',
  headroomTip: '自動圧縮はウィンドウの {pct} で発動する見込み',
  headroomBuffer: '出力予約の手前に空けておくバッファ',
  outputTip: 'モデルの 1 回あたりの最大出力 tokens',
} satisfies typeof zh
