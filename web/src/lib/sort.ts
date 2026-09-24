// 表格点列头排序：再点一次反向。数字列默认降序，文字列默认升序；空值总在最后。
import { computed, type Ref, ref } from 'vue'

type SortDir = 'asc' | 'desc'
type Val = number | string | null | undefined

export interface Sort<K extends string> {
  key: Ref<K>
  dir: Ref<SortDir>
  toggle: (k: K) => void
}

export function useSort<R, K extends string>(rows: Ref<readonly R[]>, cols: Record<K, (r: R) => Val>, init: NoInfer<K>) {
  const key = ref(init) as Ref<K>
  const dir = ref<SortDir>('desc')

  function toggle(k: K) {
    if (key.value === k) {
      dir.value = dir.value === 'desc' ? 'asc' : 'desc'
      return
    }
    key.value = k
    const sample = rows.value.map(cols[k]).find((v) => v != null)
    dir.value = typeof sample === 'string' ? 'asc' : 'desc'
  }

  const sorted = computed(() => {
    const get = cols[key.value]
    const sign = dir.value === 'asc' ? 1 : -1
    return [...rows.value].sort((a, b) => {
      const x = get(a)
      const y = get(b)
      if (x == null || y == null) return x == null ? (y == null ? 0 : 1) : -1
      if (typeof x === 'string' || typeof y === 'string') return sign * String(x).localeCompare(String(y))
      return sign * (x - y)
    })
  })

  const sort: Sort<K> = { key, dir, toggle }
  return { sort, sorted }
}

/** 长表只显示前 n 行，可以展开 */
export function useTopN<R>(rows: Ref<readonly R[]>, n: number) {
  const showAll = ref(false)
  const shown = computed(() => (showAll.value ? rows.value : rows.value.slice(0, n)))
  const more = computed(() => rows.value.length > n)
  return { shown, showAll, more }
}
