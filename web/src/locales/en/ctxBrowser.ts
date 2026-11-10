import type zh from '../zh-CN/ctxBrowser'

export default {
  filter: 'Filter name, description, args, body…',
  base: {
    label: 'Diff baseline',
    step: 'prev step',
    turn: 'prev turn',
    tip: 'Diff baseline: the previous step, or the previous turn’s last step; badges are hidden when there is nothing to compare against',
    vs: 'vs #{n}',
    none: 'Nothing to compare against',
  },
  dna: {
    aria: 'Context items as color bands in read order',
    hint: 'One band per item sized by tokens, in the order the model reads them; hover for details, click to open its row',
    tools: '{n} tool schemas',
  },
  map: {
    toggle: 'Block map',
    tip: 'Block map: regions sized by tokens, with same-kind items grouped into one block (e.g. every bash result together); click a block to show only its rows below',
    others: '{n} more',
    empty: 'Nothing to map — this step’s context is empty',
    items: '{n} items',
    share: '{p} of total',
    only: 'Only: {name}',
    clear: 'Clear',
  },
  body: {
    find: 'Find in text',
    findPlaceholder: 'Find…',
    prev: 'Previous match',
    next: 'Next match',
    close: 'Close find',
    matches: '{i}/{n}',
    noMatch: 'No matches',
    copyRaw: 'Copy raw',
    copyJson: 'Copy JSON',
    copied: 'Copied',
    copyFailed: 'Copy failed',
  },
} satisfies typeof zh
