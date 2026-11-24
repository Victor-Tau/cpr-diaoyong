<script setup lang="ts">
import { computed, onMounted, shallowRef } from 'vue'
import { applyPreview, getOptions, queryPreview } from './api'
import type { Options, Order, Preview } from './api'
import PreviewTable from './PreviewTable.vue'

const options = shallowRef<Options>()
const group = shallowRef('')
const plan = shallowRef('plus')
const order = shallowRef<Order>('startsAt')
const preview = shallowRef<Preview>()
const selected = shallowRef<string[]>([])
const pending = shallowRef<'load' | 'query' | 'apply'>()
const failure = shallowRef('')
const notice = shallowRef('')
const canQuery = computed(() => Boolean(group.value && plan.value && !pending.value))
const canApply = computed(() => Boolean(preview.value?.previewId && selected.value.length && !pending.value))

function clearPreview() {
  preview.value = undefined
  selected.value = []
  failure.value = ''
  notice.value = ''
}

async function load() {
  pending.value = 'load'
  failure.value = ''
  try {
    options.value = await getOptions()
    plan.value = options.value.planTypes.includes('plus') ? 'plus' : (options.value.planTypes[0] ?? '')
  }
  catch (error) {
    failure.value = error instanceof Error ? error.message : '加载分组失败'
  }
  finally {
    pending.value = undefined
  }
}

async function query() {
  if (!canQuery.value) return
  clearPreview()
  pending.value = 'query'
  try {
    const result = await queryPreview(group.value, plan.value, order.value)
    preview.value = result
    selected.value = result.rows.filter(row => row.suggestedWeight !== null && row.suggestedWeight !== row.currentWeight).map(row => row.id)
    if (result.rows.length === 0) notice.value = '该分组没有符合套餐的账号'
    else if (!selected.value.length) notice.value = '当前没有可应用的权重变更'
  }
  catch (error) {
    failure.value = error instanceof Error ? error.message : '查询订阅失败'
  }
  finally {
    pending.value = undefined
  }
}

async function apply() {
  const id = preview.value?.previewId
  if (!id || !canApply.value) return
  failure.value = ''
  notice.value = ''
  pending.value = 'apply'
  try {
    const result = await applyPreview(id, selected.value)
    notice.value = `已更新 ${result.applied} 个账号的权重`
  }
  catch (error) {
    failure.value = error instanceof Error ? error.message : '应用结果未确认，请重新查询当前权重'
  }
  finally {
    preview.value = undefined
    selected.value = []
    pending.value = undefined
  }
}

onMounted(load)
</script>

<template>
  <main class="panel">
    <div v-if="options" class="toolbar">
      <div class="field">
        <label for="account-group">账号分组</label>
        <select id="account-group" v-model="group" :disabled="Boolean(pending)" @change="clearPreview">
          <option disabled value="">选择分组</option>
          <option v-for="item in options.groups" :key="item.id" :value="item.id">{{ item.name }}</option>
        </select>
      </div>
      <div class="field">
        <label for="account-plan">套餐</label>
        <select id="account-plan" v-model="plan" class="plan" :disabled="Boolean(pending)" @change="clearPreview">
          <option v-for="item in options.planTypes" :key="item" :value="item">{{ item }}</option>
        </select>
      </div>
      <div class="field">
        <label for="rank-order">排序依据</label>
        <select id="rank-order" v-model="order" :disabled="Boolean(pending)" @change="clearPreview">
          <option value="startsAt">开始时间优先</option>
          <option value="expiresAt">到期时间优先</option>
        </select>
      </div>
      <button type="button" class="button" :disabled="!canQuery" @click="query">
        {{ pending === 'query' ? '查询中…' : '查询' }}
      </button>
    </div>

    <p v-if="pending === 'load'" class="status" role="status">正在加载分组…</p>
    <p v-if="options && !options.groups.length" class="status">暂无可选分组</p>
    <p v-if="options && !options.planTypes.length" class="status">请在插件设置中填写可编排的套餐</p>
    <p v-if="failure" class="status error" role="alert">{{ failure }}</p>
    <button v-if="!options && failure" type="button" class="button" @click="load">重新加载</button>
    <p v-if="notice" class="status" role="status">{{ notice }}</p>

    <template v-if="preview?.rows.length">
      <PreviewTable v-model="selected" :rows="preview.rows" :disabled="pending === 'apply'" />
      <div v-if="preview.previewId" class="actions">
        <p class="note">账号权重也会在该账号所属的其他分组生效</p>
        <button type="button" class="button" :disabled="!canApply" @click="apply">
          {{ pending === 'apply' ? '应用中…' : `应用（${selected.length}）` }}
        </button>
      </div>
    </template>
  </main>
</template>

<style>
body { margin: 0; }

.panel {
  box-sizing: border-box;
  display: grid;
  gap: 1rem;
  padding: 1rem;
  background: var(--z-color-bg-container, Canvas);
  color: var(--z-color-text, CanvasText);
  border-radius: var(--z-border-radius-lg, 12px);
  font-size: var(--z-font-size, 14px);
}

.toolbar, .actions { display: flex; align-items: end; flex-wrap: wrap; gap: .75rem; }
.field { display: grid; gap: .375rem; min-width: 9rem; max-width: 100%; }
.plan { text-transform: capitalize; }
.field select, .button {
  box-sizing: border-box;
  min-height: 2.25rem;
  max-width: 100%;
  padding: .375rem .75rem;
  border: 0;
  border-radius: var(--z-border-radius, 6px);
  background: var(--z-color-fill-secondary, ButtonFace);
  color: inherit;
  font: inherit;
}
.button {
  cursor: pointer;
  background: var(--z-color-primary, Highlight);
  color: var(--z-color-text-light-solid, HighlightText);
}
.button:hover:not(:disabled) { background: var(--z-color-primary-hover, Highlight); }
.button:disabled, .field select:disabled { opacity: .55; cursor: default; }
.button:focus-visible, .field select:focus-visible, input:focus-visible {
  outline: 2px solid var(--z-color-primary, Highlight);
  outline-offset: 2px;
}
.status, .note { margin: 0; }
.note { align-self: center; color: var(--z-color-text-secondary, GrayText); }
.error { color: var(--z-color-error-text, CanvasText); }
.actions { justify-content: space-between; }
</style>
