import { useEffect, useState } from 'react'
import { Check, ChevronRight, Cloud, Copy, Download, Eye, EyeOff, HardDrive, Info, KeyRound, Monitor, Moon, Network, Plus, RotateCcw, Save, Settings2, ShieldCheck, SlidersHorizontal, Sun, Trash2 } from 'lucide-react'
import { downloadFile, useWorkspace, useTranslation, type Settings } from '../workspace'
import { api, keyInput, policyInput } from '../api'
import type { KeyPolicy } from '../key-policy'
import { gatewayUrl } from '../gateway'
import { formatVersion, updateActionEnabled, updateButtonLabel, waitForRestart, type UpdateCheckResult } from '../self-update'
import { Toggle, SectionHeading } from '../components/UI'
import { Dropdown } from './Dropdown'
import KeyPolicyDialog, { type PreviewKey } from './KeyPolicyDialog'

type SettingsTab = 'General' | 'Gateway' | 'Routing' | 'Appearance' | 'Security' | 'About'
const tabs = [
  { name: 'General', icon: Settings2, key: 'general', description: 'generalDescription' },
  { name: 'Gateway', icon: Network, key: 'gateway', description: 'gatewayDescription' },
  { name: 'Routing', icon: SlidersHorizontal, key: 'routing', description: 'routingDefaultsDescription' },
  { name: 'Appearance', icon: Sun, key: 'appearance', description: 'appearanceDescription' },
  { name: 'Security', icon: ShieldCheck, key: 'security', description: 'securityDescription' },
  { name: 'About', icon: Info, key: 'about', description: 'aboutDescription' },
] as const
function SettingRow({ title, description, children }: { title: string; description: string; children: React.ReactNode }) {
  return <div className="setting-row"><div><h4>{title}</h4><p>{description}</p></div><div className="setting-control">{children}</div></div>
}
function Field({ label, value, onChange, type = 'text', hint, required = true }: { label: string; value: string; onChange: (value: string) => void; type?: string; hint?: string; required?: boolean }) {
  return <label className="settings-field"><span>{label}</span><input className="field" type={type} value={value} required={required} onChange={(e) => onChange(e.target.value)} />{hint && <small>{hint}</small>}</label>
}
function SelectField({ label, value, options, onChange, optionLabels = {} }: { label: string; value: string; options: string[]; onChange: (value: string) => void; optionLabels?: Record<string, string> }) {
  return <Dropdown label={label} value={value} options={options.map((option) => ({ value: option, label: optionLabels[option] || option }))} onChange={onChange} />
}
export default function SettingsView() {
  const { settings, setSettings, notify, requests, providers, routes, plugins, clientKeys, run, version } = useWorkspace()
  const t = useTranslation()
  const [tab, setTab] = useState<SettingsTab>('General')
  const [draft, setDraft] = useState(settings)
  const [revealed, setRevealed] = useState<number | null>(null)
  const [copied, setCopied] = useState<number | null>(null)
  const [createdKey, setCreatedKey] = useState<PreviewKey | null>(null)
  const [newKeyOpen, setNewKeyOpen] = useState(false)
  const [endpointCopied, setEndpointCopied] = useState(false)
  const [updateCheck, setUpdateCheck] = useState<UpdateCheckResult | null>(null)
  const [updatePhase, setUpdatePhase] = useState<'idle' | 'checking' | 'applying' | 'restarting'>('idle')
  const [updateError, setUpdateError] = useState('')
  const dirty = JSON.stringify(draft) !== JSON.stringify(settings)
  const update = <K extends keyof Settings>(key: K, value: Settings[K]) => setDraft({ ...draft, [key]: value })
  useEffect(() => { if (!dirty) setDraft(settings) }, [settings, dirty])
  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    const nextSettings = draft.deployment === 'local' ? { ...draft, protocol: 'http' as const } : draft.deployment === 'cloud' && draft.protocol === 'https' ? { ...draft, port: draft.port.trim() || '5000' } : draft
    if (!nextSettings.host.trim()) { notify(t('gatewayValidation')); return }
    const requiresPort = nextSettings.deployment === 'local' || nextSettings.protocol === 'http'
    const port = Number(nextSettings.port)
    if (requiresPort && (!Number.isInteger(port) || port < 1 || port > 65535)) { notify(t('gatewayValidation')); return }
    try { gatewayUrl(nextSettings) } catch { notify(t('gatewayValidation')); return }
    if (!Number.isInteger(Number(nextSettings.retries)) || Number(nextSettings.retries) < 1 || Number(nextSettings.retries) > 16) { notify(t('timeoutValidation')); return }
    try { await run(() => api('/api/v1/settings', { method: 'POST', body: JSON.stringify({ max_attempts: Number(nextSettings.retries), log_bodies: nextSettings.logBodies }) })); setSettings(nextSettings); setDraft(nextSettings); notify(t('settingsSaved')) } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) }
  }
  const copy = async (id: number, value: string) => { try { await navigator.clipboard.writeText(value); setCopied(id); setTimeout(() => setCopied(null), 1500) } catch { notify(t('clipboardUnavailable')) } }
  const copyEndpoint = async () => { try { await navigator.clipboard.writeText(gatewayUrl(draft)); setEndpointCopied(true); setTimeout(() => setEndpointCopied(false), 1500); notify(t('endpointCopied')) } catch { notify(t('clipboardUnavailable')) } }
  const changeDeployment = (deployment: Settings['deployment']) => setDraft({ ...draft, deployment, protocol: deployment === 'local' ? 'http' : 'https' })
  const exportWorkspace = () => { downloadFile('sepia-workspace.json', JSON.stringify({ settings, providers, routes, plugins, requests, exportedAt: new Date().toISOString() }, null, 2)); notify(t('workspaceExported')) }
  const createKey = async (policy: KeyPolicy) => {
    const created = await run(() => api<{ id: number; plaintext: string }>('/api/v1/keys', { method: 'POST', body: JSON.stringify(keyInput(policy)) }))
    try { await run(() => api(`/api/v1/keys/${created.id}/policy`, { method: 'POST', body: JSON.stringify(policyInput(policy, providers)) })) }
    catch (cause) { await run(() => api(`/api/v1/keys/${created.id}/revoke`, { method: 'POST' })).catch(() => undefined); throw cause }
    if (!policy.enabled) await run(() => api(`/api/v1/keys/${created.id}/enabled`, { method: 'POST', body: JSON.stringify({ enabled: false }) }))
    setCreatedKey({ ...policy, id: created.id, value: created.plaintext, created: t('justNow') }); setRevealed(created.id); setNewKeyOpen(false); notify(t('keyGenerated'))
  }
  const revokeKey = async (id: number) => { try { await run(() => api(`/api/v1/keys/${id}/revoke`, { method: 'POST' })); if (createdKey?.id === id) setCreatedKey(null); notify(t('keyRemoved')) } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) } }
  const checkUpdate = async () => {
    setUpdatePhase('checking'); setUpdateError('')
    try { setUpdateCheck(await api<UpdateCheckResult>('/api/v1/update/check')) }
    catch (cause) { setUpdateError(cause instanceof Error ? cause.message : String(cause)) }
    finally { setUpdatePhase('idle') }
  }
  const applyUpdate = async () => {
    if (!updateCheck?.update_available || !updateCheck.platform_supported) return
    const previousVersion = version
    setUpdatePhase('applying'); setUpdateError('')
    try {
      await api<{ version: string; restarting: boolean }>('/api/v1/update', { method: 'POST' })
      setUpdatePhase('restarting')
      const restarted = await waitForRestart(previousVersion, async () => (await api<{ version: string }>('/api/v1/version')).version)
      if (restarted) { window.location.reload(); return }
      setUpdatePhase('idle')
      setUpdateError(t('updateRestartTimeout'))
    } catch (cause) {
      setUpdatePhase('idle')
      setUpdateError(cause instanceof Error ? cause.message : String(cause))
    }
  }
  return <div className="settings-view">
    <SectionHeading eyebrow={t('homeEyebrow')} title={t('settingsTitle')} description={t('settingsSubtitle')} action={<button className="button primary" type="submit" form="settings-form" disabled={!dirty}><Save size={15} />{dirty ? t('saveChanges') : t('allSaved')}</button>} />
    <div className="settings-layout"><nav className="settings-nav glass-panel" aria-label={t('settings')}>{tabs.map(({ name, icon: Icon, key }) => <button type="button" key={name} className={tab === name ? 'active' : ''} onClick={() => setTab(name)}><Icon size={16} /><span>{t(key)}</span><ChevronRight size={13} /></button>)}<div className="settings-local-note"><ShieldCheck size={18} /><strong>{t('browserStorage')}</strong><p>{t('browserStorageDescription')}</p></div></nav>
      <form id="settings-form" className="settings-content" onSubmit={save}><div className="settings-section-heading"><h2>{t(tabs.find((item) => item.name === tab)!.key)}</h2><p>{t(tabs.find((item) => item.name === tab)!.description)}</p></div>
        {tab === 'General' && <><section className="settings-card glass-panel"><h3>{t('workspaceSection')}</h3><div className="settings-fields"><Field label={t('workspaceName')} value={draft.workspace} onChange={(v) => update('workspace', v)} /><SelectField label={t('language')} value={draft.language} options={['English', '简体中文', '日本語']} onChange={(v) => { update('language', v); setSettings({ ...settings, language: v }) }} /></div></section><section className="settings-card glass-panel"><h3>{t('preferences')}</h3><SettingRow title={t('reduceMotion')} description={t('reduceMotionDescription')}><Toggle label={t('reduceMotion')} checked={draft.reducedMotion} onChange={() => update('reducedMotion', !draft.reducedMotion)} /></SettingRow><SettingRow title={t('compactTables')} description={t('compactTablesDescription')}><Toggle label={t('compactTables')} checked={draft.compact} onChange={() => update('compact', !draft.compact)} /></SettingRow><div className="settings-version"><span>{t('frontendVersion')}</span><span>{formatVersion(version)} <i />{t('browserWorkspace')}</span></div></section><section className="settings-card glass-panel"><h3>{t('loggingSection')}</h3><SettingRow title={t('logBodies')} description={t('logBodiesDescription')}><Toggle checked={draft.logBodies} onChange={() => update('logBodies', !draft.logBodies)} label={t('logBodies')} /></SettingRow><p className="setting-note">{t('logBodiesRetentionNote')}</p></section><section className="settings-card glass-panel"><h3>{t('yourData')}</h3><SettingRow title={t('exportWorkspace')} description={t('exportWorkspaceDescription')}><button type="button" className="button secondary" onClick={exportWorkspace}><Download size={14} />{t('export')}</button></SettingRow></section></>}
        {tab === 'Gateway' && <><section className="settings-card glass-panel gateway-network"><div className="settings-card-title gateway-network-title"><div><h3>{t('network')}</h3><p>{t('gatewayDescription')}</p></div><Network size={18} /></div><div className="gateway-deployment-options" role="group" aria-label={t('deployment')}><button type="button" className={draft.deployment === 'local' ? 'selected' : ''} aria-pressed={draft.deployment === 'local'} onClick={() => changeDeployment('local')}><span className="gateway-deployment-icon"><HardDrive size={19} /></span><span><strong>{t('local')}</strong><small>{settings.language === '简体中文' ? '本机或局域网中的网关' : settings.language === '日本語' ? 'マシンまたはローカルネットワーク' : 'On your machine or network'}</small></span>{draft.deployment === 'local' && <Check size={15} />}</button><button type="button" className={draft.deployment === 'cloud' ? 'selected' : ''} aria-pressed={draft.deployment === 'cloud'} onClick={() => changeDeployment('cloud')}><span className="gateway-deployment-icon"><Cloud size={19} /></span><span><strong>{t('cloud')}</strong><small>{settings.language === '简体中文' ? '可通过公网访问的网关' : settings.language === '日本語' ? 'インターネットからアクセス' : 'Reachable over the internet'}</small></span>{draft.deployment === 'cloud' && <Check size={15} />}</button></div><div className="gateway-endpoint"><div><span>{t('gatewayEndpoint')}</span><code>{gatewayUrl(draft)}</code><small>{settings.language === '简体中文' ? '将此地址配置为你的应用 API 基础地址' : settings.language === '日本語' ? 'アプリケーションの API ベースアドレスとして使用' : 'Use this as the API base address in your application'}</small></div><button type="button" className="button secondary" onClick={copyEndpoint}>{endpointCopied ? <Check size={14} /> : <Copy size={14} />}{endpointCopied ? t('copied') : t('copyClipboard')}</button></div><div className="settings-fields gateway-fields"><label className="settings-field"><span>{t('channelProtocol')}</span>{draft.deployment === 'local' ? <div className="gateway-protocol-static"><ShieldCheck size={14} />HTTP<small>{settings.language === '简体中文' ? '本地部署固定使用' : settings.language === '日本語' ? 'ローカルでは固定' : 'Required for local deployment'}</small></div> : <div className="gateway-protocol-options" role="group" aria-label={t('channelProtocol')}><button type="button" className={draft.protocol === 'http' ? 'active' : ''} aria-pressed={draft.protocol === 'http'} onClick={() => update('protocol', 'http')}>HTTP</button><button type="button" className={draft.protocol === 'https' ? 'active' : ''} aria-pressed={draft.protocol === 'https'} onClick={() => update('protocol', 'https')}>HTTPS</button></div>}</label><div className={`gateway-address-fields ${draft.deployment === 'cloud' && draft.protocol === 'https' ? 'https-display' : ''}`}><Field label={t('host')} value={draft.host} onChange={(v) => update('host', v)} hint={draft.deployment === 'cloud' ? t('cloudHostHint') : undefined} /><Field label={t('port')} type="number" value={draft.port} onChange={(v) => update('port', v)} required={!(draft.deployment === 'cloud' && draft.protocol === 'https')} hint={draft.deployment === 'cloud' && draft.protocol === 'https' ? t('httpsPortHint') : undefined} /></div></div></section></>}
        {tab === 'Routing' && <section className="settings-card glass-panel"><h3>{t('reliability')}</h3><div className="settings-fields"><SelectField label={t('maximumAttempts')} value={draft.retries} options={Array.from({ length: 16 }, (_, index) => String(index + 1))} onChange={(v) => update('retries', v)} /></div></section>}
        {tab === 'Appearance' && <><section className="settings-card glass-panel"><h3>{t('theme')}</h3><div className="theme-options">{([['Sepia dark', Moon], ['Light', Sun], ['System', Monitor]] as [string, typeof Moon][]).map(([name, Icon]) => <button type="button" key={name as string} className={`theme-option ${draft.theme === name ? 'selected' : ''}`} onClick={() => update('theme', name as string)}><div className={`theme-preview ${name === 'Light' ? 'light-preview' : name === 'System' ? 'system-preview' : 'dark-preview'}`}><i /><span /><span /></div><span><Icon size={14} />{name === 'Sepia dark' ? t('darkTheme') : name === 'Light' ? t('lightTheme') : t('systemTheme')}{draft.theme === name && <Check size={14} />}</span></button>)}</div></section><section className="settings-card glass-panel"><h3>{t('accentColor')}</h3><div className="accent-options">{[['Sage', '#c4eaaa'], ['Sky', '#a5cfe5'], ['Lavender', '#c3b3e5'], ['Sand', '#dfc49f']].map(([name, color]) => <button type="button" key={name} className={draft.accent === name ? 'selected' : ''} onClick={() => update('accent', name)}><i style={{ background: color }} />{name === 'Sage' ? t('sage') : name === 'Sky' ? t('sky') : name === 'Lavender' ? t('lavender') : t('sand')}{draft.accent === name && <Check size={13} />}</button>)}</div></section></>}
        {tab === 'Security' && <><section className="settings-card glass-panel"><div className="settings-card-title"><div><h3>{t('previewKeys')}</h3><p>{t('previewKeysDescription')}</p></div><button type="button" className="button secondary" onClick={() => setNewKeyOpen(true)}><Plus size={14} />{t('createKey')}</button></div>{clientKeys.map((key) => { const value = createdKey?.id === key.id ? createdKey.value : ''; return <div className="api-key-row" key={key.id}><span className="key-icon"><KeyRound size={17} /></span><div><strong>{key.name}</strong><span>{key.models.length} {t('models')} · {key.rpm} RPM</span></div><code>{value && revealed === key.id ? value : 'sk-sepia-••••••••'}</code>{value && <button type="button" className="icon-button" onClick={() => setRevealed(revealed === key.id ? null : key.id)} aria-label={revealed === key.id ? t('hideApiKey') : t('showApiKey')}>{revealed === key.id ? <EyeOff size={15} /> : <Eye size={15} />}</button>}{value && <button type="button" className="icon-button" onClick={() => copy(key.id, value)} aria-label={t('copyApiKey')}>{copied === key.id ? <Check size={15} /> : <Copy size={15} />}</button>}<button type="button" className="icon-button danger-icon" onClick={() => revokeKey(key.id)} aria-label={t('deleteNamed', { name: key.name })}><Trash2 size={15} /></button></div> })}{clientKeys.length === 0 && <div className="empty-state">{t('noPreviewKeys')}</div>}<div className="settings-inline-note"><ShieldCheck size={14} />{t('keyOneTime')}</div></section><section className="settings-card glass-panel"><h3>{t('privacy')}</h3><SettingRow title={t('redaction')} description={t('redactionDescription')}><span className="always-on"><Check size={13} />{t('alwaysOn')}</span></SettingRow><SettingRow title={t('credentialHandling')} description={t('credentialHandlingDescription')}><span className="always-on"><Check size={13} />{t('protected')}</span></SettingRow></section></>}
        {tab === 'About' && <><section className="settings-card glass-panel"><div className="settings-card-title"><div><h3>{t('updates')}</h3><p>{t('updatesDescription')}</p></div><button type="button" className="button secondary" disabled={!updateActionEnabled(updatePhase, updateCheck)} onClick={() => { if (updateCheck?.update_available) void applyUpdate(); else void checkUpdate() }}>{updateButtonLabel(updatePhase, updateCheck, { check: t('updates'), checking: t('updateChecking'), apply: t('updateNow'), applying: t('updateApplying'), restarting: t('updateRestarting') })}</button></div><SettingRow title={t('currentVersion')} description={t('currentVersionDescription')}><code>{formatVersion(version)}</code></SettingRow>{updateCheck && <SettingRow title={t('latestVersion')} description={updateCheck.published_at ? t('updatePublishedAt', { date: new Date(updateCheck.published_at).toLocaleString() }) : t('latestVersionDescription')}><code>{formatVersion(updateCheck.latest_version)}</code></SettingRow>}{updateCheck && !updateCheck.update_available && <p className="setting-note">{t('updateUpToDate')}</p>}{updateCheck?.update_available && !updateCheck.platform_supported && <p className="setting-note">{t('updatePlatformUnsupported')}</p>}{updatePhase === 'restarting' && <p className="setting-note">{t('updateRestartingHint')}</p>}{updateError && <p className="setting-note update-error" role="alert">{updateError}</p>}</section>{updateCheck?.update_available && <section className="settings-card glass-panel"><h3>{t('releaseNotes')}</h3>{updateCheck.release_notes.trim() ? <pre className="release-notes">{updateCheck.release_notes}</pre> : <p className="setting-note">{t('noReleaseNotes')}</p>}{updateCheck.release_url && <a className="button text-button" href={updateCheck.release_url} target="_blank" rel="noreferrer">{t('viewRelease')}</a>}</section>}</>}
        <div className="settings-bottom"><span>{dirty ? t('unsavedChanges') : t('savedInBrowser')}</span><button type="button" className="button text-button" disabled={!dirty} onClick={() => { setDraft(settings); notify(t('changesDiscarded')) }}><RotateCcw size={13} />{t('discardChanges')}</button></div>
      </form></div>
    <KeyPolicyDialog open={newKeyOpen} onClose={() => setNewKeyOpen(false)} onCreate={createKey} />
  </div>
}
