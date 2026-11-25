import { useEffect, useMemo, useState } from 'react'
import {
  AlertCircle,
  ArrowDownRight,
  Bot,
  ChevronLeft,
  ChevronRight,
  Clock3,
  Filter,
  Search,
  X,
} from 'lucide-react'
import { downloadFile, useWorkspace, useTranslation, useLocale, type RequestRecord } from '../workspace'
import { ProviderCell, SectionHeading, StatusText } from '../components/UI'
import { Dropdown } from '../components/Dropdown'
import { api } from '../api'
import { formatRequestTime } from '../analytics'

export function RequestsTable({
  compact = false,
  rows,
  onSelect,
}: {
  compact?: boolean
  rows: RequestRecord[]
  onSelect: (row: RequestRecord) => void
}) {
  const t = useTranslation()
  const locale = useLocale()
  const formatTokens = (value: string) => Number(value.split(',').join('')).toLocaleString(locale)

  return (
    <div className="table-scroll">
      <table className="requests-table">
        <thead>
          <tr>
            <th>{t('time')}</th>
            <th>{t('model')}</th>
            <th>{t('route')}</th>
            <th>{t('provider')}</th>
            {!compact && <th>{t('apiKey')}</th>}
            <th className="num-col">{t('tokens')}</th>
            <th className="num-col">{t('latency')}</th>
            <th>{t('status')}</th>
            <th aria-label={t('requestDetails')} />
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const tone =
              row.status === 'Failed'
                ? 'danger'
                : row.status === 'Failover'
                  ? 'warning'
                  : 'positive'
            return (
              <tr key={row.id} onClick={() => onSelect(row)} className="clickable-row">
                <td className="muted mono tabular">{formatRequestTime(row.createdAt, locale)}</td>
                <td>
                  <span className="model-name mono">
                    <Bot size={13} />
                    {row.model}
                  </span>
                </td>
                <td>
                  <code className="mono">{row.route}</code>
                </td>
                <td>
                  <ProviderCell name={row.provider} />
                </td>
                {!compact && (
                  <td>
                    <span className="key-label">{row.key}</span>
                  </td>
                )}
                <td className="muted num-col tabular">{formatTokens(row.tokens)}</td>
                <td className="muted num-col tabular">{row.latency}</td>
                <td>
                  <StatusText tone={tone}>
                    {row.status === 'Failover' ? t('failover') : row.status}
                  </StatusText>
                </td>
                <td>
                  <button
                    type="button"
                    className="icon-button row-action"
                    aria-label={t('requestDetailsNamed', { id: row.id })}
                    onClick={(event) => {
                      event.stopPropagation()
                      onSelect(row)
                    }}
                  >
                    <ChevronRight size={14} />
                  </button>
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
      {rows.length === 0 && <div className="empty-state">{t('noRequestMatch')}</div>}
    </div>
  )
}

export function RequestsView({ onSelect }: { onSelect: (row: RequestRecord) => void }) {
  const { requests, notify, run, refresh } = useWorkspace()
  const t = useTranslation()
  const locale = useLocale()
  const [query, setQuery] = useState('')
  const [provider, setProvider] = useState('All providers')
  const [status, setStatus] = useState('All statuses')
  const [page, setPage] = useState(1)
  const [clearOpen, setClearOpen] = useState(false)
  const pageSize = 8

  // The request list is period-independent; analytics/overview period pickers
  // may have left the shared snapshot on a narrower window. Re-read the
  // unfiltered latest records whenever this page opens.
  useEffect(() => {
    refresh(undefined, ['requests']).catch((cause) =>
      notify(cause instanceof Error ? cause.message : String(cause)),
    )
  }, [])

  const filtered = useMemo(
    () =>
      requests.filter(
        (row) =>
          `${row.model} ${row.route} ${row.provider} ${row.key} ${row.id}`
            .toLowerCase()
            .includes(query.toLowerCase()) &&
          (provider === 'All providers' || row.provider === provider) &&
          (status === 'All statuses' || row.status === status),
      ),
    [requests, query, provider, status],
  )

  const pages = Math.max(1, Math.ceil(filtered.length / pageSize))
  const currentPage = Math.min(page, pages)

  const exportLogs = (format: 'json' | 'csv') => {
    const exported = filtered.map((row) => ({
      ...row,
      time: formatRequestTime(row.createdAt, locale),
      tokens: Number(row.tokens.split(',').join('')).toLocaleString(locale),
    }))
    if (format === 'json') {
      downloadFile('sepia-requests.json', JSON.stringify({
        scope: t('requestExportScope'),
        exported_at: new Date().toISOString(),
        count: filtered.length,
        requests: exported,
      }, null, 2))
    } else {
      const header =
        'id,time,model,route,provider,tokens,input_tokens,output_tokens,cached_tokens,reasoning_tokens,latency_ms,first_byte_ms,client_ip,status,key,cost\n'
      const escape = (value: string | number) => `"${String(value).replace(/"/g, '""')}"`
      downloadFile(
        'sepia-requests.csv',
        `# ${t('requestExportScope')}\n` + header +
          exported
            .map((row) =>
              [
                row.id,
                row.time,
                row.model,
                row.route,
                row.provider,
                row.tokens,
                row.inputTokens,
                row.outputTokens,
                row.cachedTokens,
                row.reasoningTokens,
                row.latencyMs,
                row.firstByteMs ?? '',
                row.clientIp ?? '',
                row.status,
                row.key,
                row.cost,
              ]
                .map(escape)
                .join(','),
            )
            .join('\n'),
        'text/csv',
      )
    }
    notify(`${filtered.length} ${t('requestRecordsExported')} · ${format.toUpperCase()} · ${t('requestWindowNote')}`)
  }

  return (
    <>
      <SectionHeading
        eyebrow={t('observability')}
        title={t('requestsTitle')}
        description={t('requestsSubtitle')}
        action={
          <div className="heading-actions">
            <button
              type="button"
              className="button secondary"
              onClick={() => {
                refresh(undefined, ['requests', 'analytics']).catch((cause) =>
                  notify(cause instanceof Error ? cause.message : String(cause)),
                )
              }}
            >
              {t('refresh')}
            </button>
            <button
              type="button"
              className="button secondary"
              onClick={() => exportLogs('csv')}
            >
              <ArrowDownRight size={15} />
              CSV
            </button>
            <button
              type="button"
              className="button secondary"
              onClick={() => exportLogs('json')}
            >
              <ArrowDownRight size={15} />
              JSON
            </button>
          </div>
        }
      />

      <div className="request-summary tabular">
        <span>
          {t('localHistory')}
          <span className="request-window-info" title={t('requestWindowNote')}>
            <AlertCircle size={13} aria-label={t('requestWindowNote')} />
          </span>
        </span>
        <span>
          {requests.length.toLocaleString(locale)} {t('records')} · {t('metadataShort')}
        </span>
      </div>

      <section className="requests-full glass-panel">
        <div className="request-filters">
          <div className="search-field">
            <Search size={15} />
            <input
              aria-label={t('searchRequests')}
              placeholder={t('searchRequests')}
              value={query}
              onChange={(e) => {
                setQuery(e.target.value)
                setPage(1)
              }}
            />
            {query && (
              <button
                type="button"
                className="icon-button"
                onClick={() => setQuery('')}
                aria-label={t('clearSearch')}
              >
                <X size={13} />
              </button>
            )}
          </div>

          <Dropdown
            className="filter-select"
            variant="compact"
            icon={<Filter size={13} />}
            label={t('allProviders')}
            value={provider}
            options={[
              { value: 'All providers', label: t('allProviders') },
              ...Array.from(new Set(requests.map((row) => row.provider))).map((item) => ({
                value: item,
                label: item,
              })),
            ]}
            onChange={(value) => {
              setProvider(value)
              setPage(1)
            }}
          />

          <Dropdown
            className="filter-select"
            variant="compact"
            label={t('allStatuses')}
            value={status}
            options={[
              { value: 'All statuses', label: t('allStatuses') },
              { value: '200 OK', label: '200 OK' },
              { value: 'Failed', label: t('statusFailed') },
            ]}
            onChange={(value) => {
              setStatus(value)
              setPage(1)
            }}
          />

          <span className="period-label">
            <Clock3 size={14} />
            {t('recentRequests')}
          </span>
        </div>

        <RequestsTable
          rows={filtered.slice((currentPage - 1) * pageSize, currentPage * pageSize)}
          onSelect={onSelect}
        />

        <div className="table-pagination tabular">
          <span>
            {filtered.length ? (currentPage - 1) * pageSize + 1 : 0}–
            {Math.min(currentPage * pageSize, filtered.length)} / {filtered.length}
          </span>
          <div>
            <button
              type="button"
              className="icon-button"
              disabled={currentPage === 1}
              onClick={() => setPage(currentPage - 1)}
              aria-label={t('previousPage')}
            >
              <ChevronLeft size={15} />
            </button>
            <span>
              {currentPage} / {pages}
            </span>
            <button
              type="button"
              className="icon-button"
              disabled={currentPage === pages}
              onClick={() => setPage(currentPage + 1)}
              aria-label={t('nextPage')}
            >
              <ChevronRight size={15} />
            </button>
          </div>
        </div>
      </section>

      <div className="request-clear">
        <button
          type="button"
          className="button text-button"
          onClick={() => setClearOpen(true)}
        >
          {t('clearHistory')}
        </button>
        {clearOpen && (
          <>
            <span>{t('clearHistoryConfirm')}</span>
            <button
              type="button"
              className="button text-button danger-text"
              onClick={() => {
                run(() => api('/api/v1/requests', { method: 'DELETE' }), ['requests', 'analytics'])
                  .then(() => {
                    setClearOpen(false)
                    notify(t('historyCleared'))
                  })
                  .catch((cause) =>
                    notify(cause instanceof Error ? cause.message : String(cause)),
                  )
              }}
            >
              {t('confirmClear')}
            </button>
            <button
              type="button"
              className="icon-button"
              onClick={() => setClearOpen(false)}
              aria-label={t('cancel')}
            >
              <X size={13} />
            </button>
          </>
        )}
      </div>
    </>
  )
}
