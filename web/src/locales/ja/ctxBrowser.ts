import type zh from '../zh-CN/ctxBrowser'

export default {
  filter: '名前・説明・引数・本文で絞り込み…',
  base: {
    label: '差分の基準',
    step: '前ステップ',
    turn: '前ターン',
    tip: '増減バッジの基準：前のステップ、または前ターンの最終ステップ。比較対象がない場合は表示しません',
    vs: '#{n} と比較',
    none: '比較対象なし',
  },
  dna: {
    aria: '読み取り順のコンテキスト項目カラーバンド',
    hint: '各項目を Token に比例した帯で、モデルが読む順に表示。ホバーで確認、クリックで下の行を開く',
    tools: 'ツール定義 {n} 件',
  },
  map: {
    toggle: 'ツリーマップ',
    tip: 'ツリーマップ：面積は Token に比例し、同じ種類の項目を 1 ブロックに統合（例：すべての bash 結果）。クリックで下の一覧をそのブロックの行だけに絞り込み',
    others: 'その他 {n} グループ',
    empty: 'このステップのコンテキストは空です',
    items: '{n} 項目',
    share: '全体の {p}',
    only: '絞り込み：{name}',
    clear: '解除',
  },
  body: {
    find: 'テキスト内を検索',
    findPlaceholder: '検索…',
    prev: '前の一致',
    next: '次の一致',
    close: '検索を閉じる',
    matches: '{i}/{n}',
    noMatch: '一致なし',
    copyRaw: '原文をコピー',
    copyJson: 'JSON をコピー',
    copied: 'コピー済み',
    copyFailed: 'コピー失敗',
  },
} satisfies typeof zh
