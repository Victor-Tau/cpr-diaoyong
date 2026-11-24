<script setup lang="ts">
import { ref, shallowRef, useTemplateRef } from 'vue'
import { getSettings, saveSettings } from './api'
import type { Settings } from './api'

type Draft = Omit<Settings, 'initialWeight'> & { initialWeight: number | '' }

const emit = defineEmits<{ saved: [] }>()
const dialog = useTemplateRef<HTMLDialogElement>('dialog')
const draft = ref<Draft>()
const pending = shallowRef<'load' | 'save'>()
const failure = shallowRef('')

async function load() {
  if (pending.value) return
  draft.value = undefined
  pending.value = 'load'
  failure.value = ''
  try {
    const settings = await getSettings()
    draft.value = { ...settings, initialWeight: settings.initialWeight ?? '' }
  }
  catch (error) {
    failure.value = error instanceof Error ? error.message : '加载设置失败'
  }
  finally {
    pending.value = undefined
  }
}

function open() {
  if (!dialog.value || dialog.value.open) return
  dialog.value.showModal()
  void load()
}

async function save() {
  if (!draft.value || pending.value) return
  const { initialWeight, ...settings } = draft.value
  pending.value = 'save'
  failure.value = ''
  try {
    const { changed } = await saveSettings({ ...settings, initialWeight: initialWeight === '' ? undefined : initialWeight })
    dialog.value?.close()
    if (changed) emit('saved')
  }
  catch (error) {
    failure.value = error instanceof Error ? error.message : '保存设置失败'
  }
  finally {
    pending.value = undefined
  }
}

defineExpose({ open })
</script>

<template>
  <dialog ref="dialog" class="settings-dialog" aria-labelledby="settings-title" @cancel="pending === 'save' && $event.preventDefault()">
    <form class="settings-form" @submit.prevent="save">
      <header class="dialog-header">
        <h2 id="settings-title">编排设置</h2>
      </header>

      <div class="dialog-body">
        <p v-if="pending === 'load'" class="status" role="status">正在加载设置…</p>
        <fieldset v-if="draft" class="settings-fields" :disabled="Boolean(pending)">
          <label>
            可编排的分组 ID
            <input v-model.trim="draft.groupIds" type="text" spellcheck="false" aria-describedby="group-help">
            <small id="group-help">用逗号分隔分组 ID，留空可选任意启用的分组。</small>
          </label>
          <label>
            可编排的套餐
            <input v-model.trim="draft.planTypes" type="text" required spellcheck="false" placeholder="plus,pro" aria-describedby="plan-help">
            <small id="plan-help">多个套餐用逗号分隔。</small>
          </label>
          <label>
            停用编排的账号 ID
            <textarea v-model.trim="draft.excludedAccountIds" rows="2" spellcheck="false" placeholder="acct_…" aria-describedby="exclude-help" />
            <small id="exclude-help">填写完整 ID，用逗号或换行分隔。删除 ID 即可恢复。</small>
          </label>

          <div class="settings-pair">
            <label>
              最低权重
              <input v-model="draft.minWeight" type="number" min="1" :max="draft.maxWeight || 100" required>
            </label>
            <label>
              最高权重
              <input v-model="draft.maxWeight" type="number" :min="draft.minWeight || 1" max="100" required>
            </label>
          </div>
          <div class="settings-pair">
            <label>
              订阅满多少天后排序
              <input v-model="draft.rankingAfterDays" type="number" min="0" max="65535" required aria-describedby="days-help">
              <small id="days-help">每 24 小时算一天，0 表示立即排序。</small>
            </label>
            <label>
              未满天数时的权重
              <input v-model="draft.initialWeight" type="number" :min="draft.rankingAfterDays > 0 ? draft.minWeight : 1" :max="draft.rankingAfterDays > 0 ? draft.maxWeight : 100" aria-describedby="initial-help">
              <small id="initial-help">留空使用最低权重。</small>
            </label>
          </div>
        </fieldset>
        <p v-if="failure" class="status error" role="alert">{{ failure }}</p>
        <button v-if="!draft && failure" class="button secondary" type="button" @click="load">重新加载</button>
      </div>

      <footer class="dialog-actions">
        <button class="button secondary" type="button" :disabled="pending === 'save'" @click="dialog?.close()">取消</button>
        <button class="button" type="submit" :disabled="!draft || Boolean(pending)">{{ pending === 'save' ? '保存中…' : '保存设置' }}</button>
      </footer>
    </form>
  </dialog>
</template>

<style scoped>
.settings-dialog {
  --settings-top: clamp(.75rem, calc((var(--plugin-viewport-height, 100dvh) - 32rem) / 2), 5rem);
  position: fixed;
  inset: var(--settings-top) 0 auto;
  box-sizing: border-box;
  width: min(32rem, calc(100% - 1.5rem));
  max-height: calc(100% - 1.5rem);
  margin: 0 auto;
  padding: 0;
  border: 1px solid var(--z-color-border-secondary, GrayText);
  border-radius: var(--z-border-radius-lg, 12px);
  background: var(--z-color-bg-elevated, Canvas);
  color: var(--z-color-text, CanvasText);
  box-shadow: 0 12px 40px rgb(0 0 0 / 24%);
}
.settings-dialog::backdrop { background: rgb(0 0 0 / 42%); }
.settings-form {
  display: flex;
  flex-direction: column;
  margin: 0;
  max-height: max(10rem, calc(var(--plugin-viewport-height, 100dvh) - var(--settings-top) - .75rem));
}
.dialog-header, .dialog-actions { padding: .875rem 1.25rem; }
.dialog-header h2 { margin: 0; font-size: 1.125rem; }
.dialog-body { display: grid; gap: .625rem; overflow-y: auto; padding: .375rem 1.25rem .875rem; }
.settings-fields { display: grid; gap: .875rem; min-width: 0; margin: 0; padding: 0; border: 0; }
.settings-fields label { display: grid; align-content: start; gap: .25rem; min-width: 0; }
.settings-fields small { color: var(--z-color-text-secondary, GrayText); }
.settings-pair { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: .75rem; }
.settings-fields input, .settings-fields textarea {
  box-sizing: border-box;
  width: 100%;
  min-height: 2rem;
  padding: .375rem .5rem;
  border: 1px solid var(--z-color-border, GrayText);
  border-radius: var(--z-border-radius, 6px);
  background: var(--z-color-bg-container, Canvas);
  color: inherit;
  font: inherit;
}
.settings-fields textarea { resize: vertical; }
.dialog-actions {
  display: flex;
  justify-content: end;
  gap: .5rem;
  border-top: 1px solid var(--z-color-border-secondary, GrayText);
}
@media (max-width: 540px) {
  .settings-dialog { width: calc(100% - 2.5rem); margin-inline: 2rem .5rem; }
}
@media (max-width: 320px) {
  .settings-pair { grid-template-columns: 1fr; }
  .dialog-header, .dialog-body, .dialog-actions { padding-inline: .875rem; }
}
</style>
