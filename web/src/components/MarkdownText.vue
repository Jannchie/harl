<script setup lang="ts">
import { computed } from 'vue'
import MarkdownIt from 'markdown-it'

// html: false —— assistant 文本里常有 <tag> 样的东西，原样转义显示，不当 HTML 解析
const md = new MarkdownIt({ html: false, linkify: true, breaks: false })
// 只把带协议的 URL 变成链接；否则 `claude.rs`、`store.rs` 这种文件名会被当成域名
md.linkify.set({ fuzzyLink: false, fuzzyEmail: false })

// 外链新开标签
const defaultLink =
  md.renderer.rules.link_open ?? ((tokens, idx, options, _env, self) => self.renderToken(tokens, idx, options))
md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
  tokens[idx]?.attrSet('target', '_blank')
  tokens[idx]?.attrSet('rel', 'noopener')
  return defaultLink(tokens, idx, options, env, self)
}

const props = defineProps<{ text: string }>()
const html = computed(() => md.render(props.text))
</script>

<template>
  <div class="chat-prose" v-html="html"></div>
</template>
