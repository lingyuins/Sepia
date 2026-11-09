import { useMemo, useState } from 'react'
import { ArrowDownRight, Bot, ChevronLeft, ChevronRight, Clock3, Filter, Search, X } from 'lucide-react'
import { downloadFile, useWorkspace, useTranslation, type RequestRecord } from '../workspace'
import { ProviderCell, SectionHeading, StatusDot } from './ProvidersView'
import { Dropdown } from '../components/Dropdown'

export function RequestsTable({ compact = false, rows, onSelect }: { compact?: boolean; rows: RequestRecord[]; onSelect: (row: RequestRecord) => void }) {
  const t = useTranslation()
  return <div className="table-scroll"><table className="requests-table"><thead><tr><th>{t('time')}</th><th>{t('model')}</th><th>{t('provider')}</th>{!compact && <th>{t('apiKey')}</th>}<th>{t('tokens')}</th><th>{t('latency')}</th><th>{t('status')}</th><th /></tr></thead><tbody>{rows.map((row) => <tr key={row.id} onClick={() => onSelect(row)} className="clickable-row"><td className="muted mono">{row.time}</td><td><span className="model-name"><Bot size={13} />{row.model}</span></td><td><ProviderCell name={row.provider} /></td>{!compact && <td><span className="key-label">{row.key === 'Development' ? t('development') : row.key === 'Production' ? t('production') : row.key}</span></td>}<td className="muted">{row.tokens}</td><td className="muted">{row.latency}</td><td><span className={`status-pill ${row.status === 'Failover' ? 'warning' : ''}`}><StatusDot />{row.status === 'Failover' ? t('failover') : row.status}</span></td><td><button className="icon-button row-action" aria-label={t('requestDetailsNamed', { id: row.id })} onClick={(event) => { event.stopPropagation(); onSelect(row) }}><ChevronRight size={14} /></button></td></tr>)}</tbody></table>{rows.length === 0 && <div className="empty-state">{t('noRequestMatch')}</div>}</div>
}

export function RequestsView({ onSelect }: { onSelect: (row: RequestRecord) => void }) {
  const { requests, notify, setRequests } = useWorkspace()
  const t = useTranslation()
  const [query, setQuery] = useState('')
  const [provider, setProvider] = useState('All providers')
  const [status, setStatus] = useState('All statuses')
  const [page, setPage] = useState(1)
  const [clearOpen, setClearOpen] = useState(false)
  const pageSize = 8
  const filtered = useMemo(() => requests.filter((row) => `${row.model} ${row.provider} ${row.key} ${row.id}`.toLowerCase().includes(query.toLowerCase()) && (provider === 'All providers' || row.provider === provider) && (status === 'All statuses' || row.status === status)), [requests, query, provider, status])
  const pages = Math.max(1, Math.ceil(filtered.length / pageSize))
  const currentPage = Math.min(page, pages)
  const exportLogs = (format: 'json' | 'csv') => {
    if (format === 'json') downloadFile('sepia-requests.json', JSON.stringify(filtered, null, 2))
    else {
      const header = 'id,time,model,provider,tokens,latency_ms,status,key,cost\n'
      const escape = (value: string | number) => `"${String(value).replace(/"/g, '""')}"`
      downloadFile('sepia-requests.csv', header + filtered.map((row) => [row.id, row.time, row.model, row.provider, row.tokens, row.latencyMs, row.status, row.key, row.cost].map(escape).join(',')).join('\n'), 'text/csv')
    }
    notify(`${filtered.length} ${t('requestRecordsExported')} · ${format.toUpperCase()}`)
  }
  return <>
    <SectionHeading eyebrow={t('observability')} title={t('requestsTitle')} description={t('requestsSubtitle')} action={<div className="heading-actions"><button className="button secondary" onClick={() => exportLogs('csv')}><ArrowDownRight size={15} />CSV</button><button className="button secondary" onClick={() => exportLogs('json')}><ArrowDownRight size={15} />JSON</button></div>} />
    <div className="request-summary"><span><StatusDot />{t('localHistory')}</span><span>{requests.length} {t('records')} · {t('metadataShort')}</span></div>
    <section className="requests-full glass-panel"><div className="request-filters"><div className="search-field"><Search size={15} /><input aria-label={t('searchRequests')} placeholder={t('searchRequests')} value={query} onChange={(e) => { setQuery(e.target.value); setPage(1) }} />{query && <button className="icon-button" onClick={() => setQuery('')} aria-label={t('clearSearch')}><X size={13} /></button>}</div><Dropdown className="filter-select" variant="compact" icon={<Filter size={13} />} label={t('allProviders')} value={provider} options={[{ value: 'All providers', label: t('allProviders') }, ...Array.from(new Set(requests.map((row) => row.provider))).map((item) => ({ value: item, label: item }))]} onChange={(value) => { setProvider(value); setPage(1) }} /><Dropdown className="filter-select" variant="compact" label={t('allStatuses')} value={status} options={[{ value: 'All statuses', label: t('allStatuses') }, { value: '200 OK', label: '200 OK' }, { value: 'Failover', label: t('failover') }]} onChange={(value) => { setStatus(value); setPage(1) }} /><span className="period-label"><Clock3 size={14} />{t('last24h')}</span></div><RequestsTable rows={filtered.slice((currentPage - 1) * pageSize, currentPage * pageSize)} onSelect={onSelect} /><div className="table-pagination"><span>{filtered.length ? (currentPage - 1) * pageSize + 1 : 0}–{Math.min(currentPage * pageSize, filtered.length)} / {filtered.length}</span><div><button className="icon-button" disabled={currentPage === 1} onClick={() => setPage(currentPage - 1)} aria-label={t('previousPage')}><ChevronLeft size={15} /></button><span>{currentPage} / {pages}</span><button className="icon-button" disabled={currentPage === pages} onClick={() => setPage(currentPage + 1)} aria-label={t('nextPage')}><ChevronRight size={15} /></button></div></div></section>
    <div className="request-clear"><button className="button text-button" onClick={() => setClearOpen(true)}>{t('clearHistory')}</button>{clearOpen && <><span>{t('clearHistoryConfirm')}</span><button className="button text-button danger-text" onClick={() => { setRequests([]); setClearOpen(false); notify(t('historyCleared')) }}>{t('confirmClear')}</button><button className="icon-button" onClick={() => setClearOpen(false)} aria-label={t('cancel')}><X size={13} /></button></>}</div>
  </>
}
