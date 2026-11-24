<script setup lang="ts">
import type { Row, Status } from './api'

defineProps<{ rows: Row[], disabled: boolean }>()
const selected = defineModel<string[]>({ required: true })

const labels: Record<Status, string> = {
  active: '订阅有效',
  expired: '周期已结束',
  unknown: '订阅信息不足',
  excluded: '已停用编排',
  disabled: '账号已停用',
  unsupported: '不支持订阅查询',
}
</script>

<template>
  <div class="table-wrap" role="region" aria-label="权重预览" tabindex="0">
    <table class="preview-table">
      <thead>
        <tr>
          <th scope="col">应用</th>
          <th scope="col">账号</th>
          <th scope="col">订阅开始</th>
          <th scope="col">订阅到期</th>
          <th scope="col">当前权重</th>
          <th scope="col">建议权重</th>
          <th scope="col">状态</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in rows" :key="row.id">
          <td>
            <input
              v-if="row.suggestedWeight !== null && row.suggestedWeight !== row.currentWeight"
              v-model="selected"
              type="checkbox"
              :value="row.id"
              :disabled="disabled"
              :aria-label="`应用 ${row.label} 的建议权重`"
            >
            <span v-else>—</span>
          </td>
          <th scope="row" class="account">
            {{ row.label }}
            <small v-if="row.id !== row.label" class="account-id">{{ row.id }}</small>
          </th>
          <td>{{ row.startsAt ?? '—' }}</td>
          <td>{{ row.expiresAt ?? '—' }}</td>
          <td>{{ row.currentWeight }}</td>
          <td>{{ row.suggestedWeight ?? '—' }}</td>
          <td>{{ labels[row.status] }}</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

<style scoped>
.table-wrap { overflow-x: auto; }
.preview-table { width: 100%; border-collapse: collapse; text-align: left; }
.preview-table th, .preview-table td { padding: .625rem .75rem; white-space: nowrap; }
.preview-table thead { background: var(--z-table-header-bg, ButtonFace); }
.preview-table tbody tr:nth-child(even) { background: var(--z-table-row-stripe-bg, transparent); }
.preview-table tbody tr:hover { background: var(--z-table-row-hover-bg, transparent); }
.preview-table .account { min-width: 9rem; max-width: 20rem; white-space: normal; overflow-wrap: anywhere; }
.account-id { display: block; font-weight: normal; color: var(--z-color-text-secondary, GrayText); }
.preview-table input { accent-color: var(--z-color-primary, Highlight); }

@media (max-width: 740px) {
  .preview-table th, .preview-table td { padding: .5rem .375rem; white-space: normal; }
  .preview-table .account { min-width: 6.25rem; }
}
</style>
