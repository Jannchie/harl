// 会往前走的「此刻」：「进行中」「几分钟前」这类按时间算的东西靠它隔一会儿重算一次
import { onUnmounted, ref, type Ref } from 'vue'

/** 每 intervalMs 毫秒换一次的 Date.now()；组件卸载时停掉 */
export function useNow(intervalMs: number): Readonly<Ref<number>> {
  const now = ref(Date.now())
  const timer = setInterval(() => (now.value = Date.now()), intervalMs)
  onUnmounted(() => clearInterval(timer))
  return now
}
