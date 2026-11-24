interface HostRequest {
  method: 'GET' | 'POST'
  path: string
  contentType?: string
  body?: string
}

interface HostResponse {
  status: number
  contentType: string
  body: ArrayBuffer
}

declare global {
  interface Window {
    readonly codexProxyPlugin?: {
      request: (request: HostRequest) => Promise<HostResponse>
    }
  }
}

export interface Options {
  groups: { id: string, name: string }[]
  planTypes: string[]
  weightPolicy: {
    minWeight: number
    maxWeight: number
    rankingAfterDays: number
    initialWeight: number
  }
}

export type Order = 'startsAt' | 'expiresAt'
export type Status = 'active' | 'initial' | 'expired' | 'unknown' | 'excluded' | 'disabled' | 'unsupported'

export interface Row {
  id: string
  label: string
  currentWeight: number
  suggestedWeight: number | null
  startsAt: string | null
  expiresAt: string | null
  status: Status
}

export interface Preview {
  rows: Row[]
  previewId: string | null
}

async function request<T>(path: string, data?: object): Promise<T> {
  const host = window.codexProxyPlugin
  if (!host) throw new Error('请从 Codex Proxy RS 管理端打开插件')
  const response = await host.request(data === undefined
    ? { method: 'GET', path }
    : { method: 'POST', path, contentType: 'application/json', body: JSON.stringify(data) })
  if (response.contentType.split(';', 1)[0] !== 'application/json') {
    throw new Error(`管理请求失败（HTTP ${response.status}）`)
  }
  const value = JSON.parse(new TextDecoder().decode(response.body))
  if (response.status < 200 || response.status >= 300) {
    throw new Error(typeof value?.message === 'string' ? value.message : `管理请求失败（HTTP ${response.status}）`)
  }
  return value as T
}

export const getOptions = () => request<Options>('options')

export const queryPreview = (groupId: string, planType: string, order: Order) =>
  request<Preview>('preview', { groupId, planType, order })

export const applyPreview = (previewId: string, accountIds: string[]) =>
  request<{ applied: number }>('apply', { previewId, accountIds })
