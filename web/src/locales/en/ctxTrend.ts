import type zh from '../zh-CN/ctxTrend'

export default {
  view: 'View',
  overview: 'Overview',
  overviewTip:
    'Fit every step/turn across the card width: no horizontal scrolling; flags mark the quartiles and the full span. Hover to read, click a category band to pin it and expand it in the browser; exclusive with DNA mode and Adaptive',
  adaptive: 'Adaptive',
  adaptiveTip: 'Rescale bar heights to the visible range, following the slider',
  dna: 'DNA',
  dnaTip:
    "Draw each bar as that request's context fingerprint — one band per item in the order the model reads it; with Delta, each band shows that item's change against the previous bar. Hover for the item and light up its lifetime across bars, click to reveal it in the browser",
  autoOverview: 'Auto overview',
  autoOverviewTip: 'Open in Overview instead of the slider when a session has more requests than this',
  autoSteps: '> {n} requests',
  autoOff: 'Off',
  range: '#{a}–#{b} of {n}',
  scrub: 'Visible range',
  pinned: 'Pinned: {cat}',
  unpin: 'Unpin',
  overviewHint: 'Hover to preview a request, click a category band to pin it and expand it in the browser',
  dnaHint: "Hover to light up an item's lifetime, click to reveal it in the browser",
  entered: 'Entered',
  left: 'Left',
  grew: 'Grew',
  shrank: 'Shrank',
  approxEst: 'Estimate',
  locateHint: 'Click to reveal in the browser',
  unlogged: 'Not logged (harness overhead, etc.)',
  approximate: 'This request follows a whole-context compaction; its makeup is approximate',
} satisfies typeof zh
