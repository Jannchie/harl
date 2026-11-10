import type zh from '../zh-CN/ctxTrend'

export default {
  view: '表示',
  overview: '全体',
  overviewTip:
    'セッション全体をカードの幅に収めます：横スクロール不要、旗は四分位と両端を示します。ホバーで確認、カテゴリ帯をクリックで固定してブラウザで展開。DNA・自動スケールとは併用できません',
  adaptive: '自動スケール',
  adaptiveTip: '表示範囲に合わせて棒の高さを再スケールし、スライダーに追従します',
  dna: 'DNA',
  dnaTip:
    '各棒をそのリクエストのコンテキストの指紋として描きます：モデルが読む順に項目ごとに 1 本の色帯。差分では各帯が前の棒からの増減を表します。ホバーで項目と各棒での存続を強調、クリックでブラウザに表示',
  autoOverview: '自動で全体表示',
  autoOverviewTip: 'リクエスト数がこの値を超えると、スライダーではなく全体表示で開きます',
  autoSteps: '> {n} リクエスト',
  autoOff: 'オフ',
  range: '#{a}–#{b} / {n}',
  scrub: '表示範囲',
  pinned: '固定中：{cat}',
  unpin: '固定を解除',
  overviewHint: 'ホバーでリクエストをプレビュー、カテゴリ帯をクリックで固定してブラウザで展開',
  dnaHint: 'ホバーで同じ項目の存続を強調、クリックでブラウザに表示',
  entered: '追加',
  left: '除去',
  grew: '増加',
  shrank: '減少',
  approxEst: '推定',
  locateHint: 'クリックでブラウザに表示',
  unlogged: '記録されていない部分（ハーネスのオーバーヘッドなど）',
  approximate: 'このリクエストはコンテキスト全体の圧縮の後なので、構成は近似です',
} satisfies typeof zh
