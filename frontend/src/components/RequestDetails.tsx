import { useEffect, useState } from 'react'
import { Copy, ShieldCheck } from 'lucide-react'
import { useTranslation, useWorkspace, useLocale, type RequestRecord } from '../workspace'
import { formatRequestTime } from '../analytics'
import { api } from '../api'
import { ProviderCell, StatusText } from './UI'

type Bodies = { request_body: string; response_body: string | null }

function formatTps(row: RequestRecord): string {
  if (row.firstByteMs === null || row.outputTokens <= 0) return '—'
  const generationMs = row.latencyMs - row.firstByteMs
  if (generationMs <= 0) return '—'
  return `${((row.outputTokens / generationMs) * 1000).toFixed(1)} tk/s`
}

function cacheHitRate(row: RequestRecord): string {
  if (row.cachedTokens <= 0) return '—'
  const promptTokens =
    row.protocol === 'anthropic' ? row.inputTokens + row.cachedTokens : row.inputTokens
  if (promptTokens <= 0) return '—'
  return `${((row.cachedTokens / promptTokens) * 100).toFixed(1)}%`
}

function BodyViewer({ request }: { request: RequestRecord }) {
  const t = useTranslation()
  const { notify } = useWorkspace()
  const [bodies, setBodies] = useState<Bodies | null>(null)
  const [error, setError] = useState('')
  const [tab, setTab] = useState<'request' | 'response'>('request')

  useEffect(() => {
    let active = true
    setBodies(null)
    setError('')
    setTab('request')
    api<Bodies>(`/api/v1/requests/${request.id}/bodies`)
      .then((value) => {
        if (active) setBodies(value)
      })
      .catch((cause) => {
        if (active) setError(cause instanceof Error ? cause.message : String(cause))
      })
    return () => {
      active = false
    }
  }, [request.id])

  const text = tab === 'request' ? bodies?.request_body : bodies?.response_body
  const pretty = (() => {
    if (!text) return ''
    try {
      return JSON.stringify(JSON.parse(text), null, 2)
    } catch {
      return text
    }
  })()

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(pretty)
      notify(t('copied'))
    } catch {
      notify(t('clipboardUnavailable'))
    }
  }

  return (
    <div className="body-viewer">
      <div className="body-viewer-tabs" role="tablist" aria-label={t('requestBody')}>
        <button
          type="button"
          role="tab"
          aria-selected={tab === 'request'}
          className={tab === 'request' ? 'active' : ''}
          onClick={() => setTab('request')}
        >
          {t('requestBody')}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === 'response'}
          className={tab === 'response' ? 'active' : ''}
          onClick={() => setTab('response')}
        >
          {t('responseBody')}
        </button>
        <button
          type="button"
          className="icon-button"
          onClick={copy}
          disabled={!pretty}
          aria-label={t('copyBody')}
        >
          <Copy size={14} />
        </button>
      </div>
      {error ? (
        <p className="body-viewer-empty">
          {t('bodiesLoadFailed')}: {error}
        </p>
      ) : pretty ? (
        <pre className="body-viewer-content mono">{pretty}</pre>
      ) : (
        <p className="body-viewer-empty">{bodies ? t('bodiesUnavailable') : '…'}</p>
      )}
    </div>
  )
}

export function RequestDetails({ request }: { request: RequestRecord }) {
  const t = useTranslation()
  const locale = useLocale()
  const statusTone =
    request.status === 'Failed'
      ? 'danger'
      : request.status === 'Failover'
        ? 'warning'
        : 'positive'

  return (
    <>
      <dl className="request-details">
        <div className="request-detail-wide">
          <dt>{t('requestId')}</dt>
          <dd className="mono">{request.id}</dd>
        </div>
        <div>
          <dt>{t('time')}</dt>
          <dd className="mono tabular">{formatRequestTime(request.createdAt, locale)}</dd>
        </div>
        <div>
          <dt>{t('model')}</dt>
          <dd className="mono">{request.model}</dd>
        </div>
        <div>
          <dt>{t('provider')}</dt>
          <dd>
            <ProviderCell name={request.provider} />
          </dd>
        </div>
        <div>
          <dt>{t('route')}</dt>
          <dd className="mono">{request.route}</dd>
        </div>
        <div>
          <dt>{t('apiKey')}</dt>
          <dd>{request.key}</dd>
        </div>
        {request.clientIp && (
          <div>
            <dt>{t('clientIp')}</dt>
            <dd className="mono">{request.clientIp}</dd>
          </div>
        )}
        <div className="request-detail-wide">
          <dt>{t('tokensDetail')}</dt>
          <dd className="tabular">
            {t('inputOutput', {
              input: request.inputTokens.toLocaleString(locale),
              output: request.outputTokens.toLocaleString(locale),
            })}
          </dd>
        </div>
        <div>
          <dt>{t('totalTokens')}</dt>
          <dd className="tabular">
            {(request.inputTokens + request.outputTokens).toLocaleString(locale)}
          </dd>
        </div>
        <div>
          <dt>{t('cachedTokens')}</dt>
          <dd className="tabular">
            {request.cachedTokens.toLocaleString(locale)} · {cacheHitRate(request)}
          </dd>
        </div>
        <div>
          <dt>{t('reasoningTokens')}</dt>
          <dd className="tabular">{request.reasoningTokens.toLocaleString(locale)}</dd>
        </div>
        {request.firstByteMs !== null && (
          <div>
            <dt>{t('firstByte')}</dt>
            <dd className="tabular">{request.firstByteMs} ms</dd>
          </div>
        )}
        <div>
          <dt>{t('tps')}</dt>
          <dd className="tabular">{formatTps(request)}</dd>
        </div>
        <div>
          <dt>{t('latency')}</dt>
          <dd className="tabular">{request.latency}</dd>
        </div>
        <div>
          <dt>{t('estimatedCost')}</dt>
          <dd className="tabular">{request.cost}</dd>
        </div>
        <div>
          <dt>{t('status')}</dt>
          <dd>
            <StatusText tone={statusTone}>{request.status}</StatusText>
          </dd>
        </div>
      </dl>
      {request.hasBody && <BodyViewer request={request} />}
      <div className="secure-note">
        <ShieldCheck size={15} />
        {request.hasBody ? t('bodiesRecorded') : t('metadataOnly')}
      </div>
    </>
  )
}
