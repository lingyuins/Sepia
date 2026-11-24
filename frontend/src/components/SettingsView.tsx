import { useEffect, useState } from 'react'
import {
  Bot,
  Check,
  ChevronRight,
  Cloud,
  Copy,
  Download,
  Eye,
  EyeOff,
  Github,
  HardDrive,
  Info,
  KeyRound,
  Monitor,
  Moon,
  Network,
  Plus,
  RotateCcw,
  Save,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Sun,
  Trash2,
} from 'lucide-react'
import { downloadFile, useWorkspace, useTranslation, useLocale, type Settings } from '../workspace'
import { api, keyInput, policyInput } from '../api'
import type { KeyPolicy } from '../key-policy'
import { gatewayUrl } from '../gateway'
import {
  formatVersion,
  updateActionEnabled,
  updateButtonLabel,
  waitForRestart,
  type UpdateCheckResult,
} from '../self-update'
import { Toggle, SectionHeading } from '../components/UI'
import { Dropdown } from './Dropdown'
import KeyPolicyDialog, { type PreviewKey } from './KeyPolicyDialog'
import ModelNormalizeEditor from './ModelNormalizeEditor'
import { validateNormalizeRules } from '../model-normalize'
import type { TranslationKey } from '../i18n'
import { PasskeySection, UserManagementSection } from './UserSecurity'
import { AiPoolSection } from './AiPoolSection'

type SettingsTab =
  | 'General'
  | 'Gateway'
  | 'Routing'
  | 'AI'
  | 'Appearance'
  | 'Security'
  | 'About'

const tabs = [
  { name: 'General', icon: Settings2, key: 'general', description: 'generalDescription' },
  { name: 'Gateway', icon: Network, key: 'gateway', description: 'gatewayDescription' },
  {
    name: 'Routing',
    icon: SlidersHorizontal,
    key: 'routing',
    description: 'routingDefaultsDescription',
  },
  { name: 'AI', icon: Bot, key: 'ai', description: 'aiDescription' },
  { name: 'Appearance', icon: Sun, key: 'appearance', description: 'appearanceDescription' },
  { name: 'Security', icon: ShieldCheck, key: 'security', description: 'securityDescription' },
  { name: 'About', icon: Info, key: 'about', description: 'aboutDescription' },
] as const

const serverSettingsKeys: (keyof Settings)[] = [
  'workspace',
  'deployment',
  'protocol',
  'host',
  'port',
  'retries',
  'connectTimeout',
  'firstByteTimeout',
  'streamIdleTimeout',
  'retryAfterCap',
  'logBodies',
  'normalizePrefixes',
  'normalizeSuffixes',
  'normalizeMappings',
  'aiPoolEnabled',
]

function SettingRow({
  title,
  description,
  children,
}: {
  title: string
  description: string
  children: React.ReactNode
}) {
  return (
    <div className="setting-row">
      <div>
        <h4>{title}</h4>
        <p>{description}</p>
      </div>
      <div className="setting-control">{children}</div>
    </div>
  )
}

function SettingsField({
  label,
  value,
  onChange,
  type = 'text',
  hint,
  required = true,
  disabled = false,
}: {
  label: string
  value: string
  onChange: (value: string) => void
  type?: string
  hint?: string
  required?: boolean
  disabled?: boolean
}) {
  return (
    <label className="settings-field">
      <span>{label}</span>
      <input
        className="field"
        type={type}
        value={value}
        required={required}
        onChange={(e) => onChange(e.target.value)}
        disabled={disabled}
      />
      {hint && <small>{hint}</small>}
    </label>
  )
}

function SelectField({
  label,
  value,
  options,
  onChange,
  optionLabels = {},
}: {
  label: string
  value: string
  options: string[]
  onChange: (value: string) => void
  optionLabels?: Record<string, string>
}) {
  return (
    <Dropdown
      label={label}
      value={value}
      options={options.map((option) => ({ value: option, label: optionLabels[option] || option }))}
      onChange={onChange}
    />
  )
}

export default function SettingsView() {
  const {
    settings,
    setSettings,
    notify,
    requests,
    providers,
    routes,
    plugins,
    clientKeys,
    run,
    version,
    session,
  } = useWorkspace()
  const t = useTranslation()
  const locale = useLocale()
  const [tab, setTab] = useState<SettingsTab>('General')
  const isAdmin = session?.role === 'admin'
  const [draft, setDraft] = useState(settings)
  const [revealed, setRevealed] = useState<number | null>(null)
  const [copied, setCopied] = useState<number | null>(null)
  const [createdKey, setCreatedKey] = useState<PreviewKey | null>(null)
  const [newKeyOpen, setNewKeyOpen] = useState(false)
  const [endpointCopied, setEndpointCopied] = useState(false)
  const [updateCheck, setUpdateCheck] = useState<UpdateCheckResult | null>(null)
  const [updatePhase, setUpdatePhase] = useState<'idle' | 'checking' | 'applying' | 'restarting'>(
    'idle',
  )
  const [updateError, setUpdateError] = useState('')
  const [aiPoolBusy, setAiPoolBusy] = useState(false)

  const dirty = JSON.stringify(draft) !== JSON.stringify(settings)
  const update = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    setDraft({ ...draft, [key]: value })

  useEffect(() => {
    if (!dirty) setDraft(settings)
  }, [settings, dirty])

  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    const nextSettings =
      draft.deployment === 'local'
        ? { ...draft, protocol: 'http' as const }
        : draft.deployment === 'cloud' && draft.protocol === 'https'
          ? { ...draft, port: draft.port.trim() || '5000' }
          : draft

    if (!nextSettings.host.trim()) {
      notify(t('gatewayValidation'))
      return
    }
    const requiresPort = nextSettings.deployment === 'local' || nextSettings.protocol === 'http'
    const port = Number(nextSettings.port)
    if (requiresPort && (!Number.isInteger(port) || port < 1 || port > 65535)) {
      notify(t('gatewayValidation'))
      return
    }
    try {
      gatewayUrl(nextSettings)
    } catch {
      notify(t('gatewayValidation'))
      return
    }
    const numericRanges: [keyof Settings, number][] = [
      ['retries', 16],
      ['connectTimeout', 60],
      ['firstByteTimeout', 600],
      ['streamIdleTimeout', 600],
      ['retryAfterCap', 60],
    ]
    if (
      numericRanges.some(([key, max]) => {
        const value = Number(nextSettings[key])
        return !Number.isInteger(value) || value < 1 || value > max
      })
    ) {
      notify(t('timeoutValidation'))
      return
    }
    const ruleError = validateNormalizeRules({
      routerPrefixes: nextSettings.normalizePrefixes,
      functionalSuffixes: nextSettings.normalizeSuffixes,
      explicitMappings: nextSettings.normalizeMappings,
    })
    if (ruleError) {
      notify(t(ruleError.key as TranslationKey, ruleError.params))
      return
    }

    const normalized = { ...nextSettings, workspace: nextSettings.workspace.trim(), host: nextSettings.host.trim(), port: nextSettings.port.trim() }
    const serverChanged = serverSettingsKeys.some(
      (key) => JSON.stringify(normalized[key]) !== JSON.stringify(settings[key]),
    )
    const persisted: Settings = isAdmin
      ? normalized
      : serverSettingsKeys.reduce(
          (merged, key) => ({ ...merged, [key]: settings[key] }),
          normalized as Settings,
        )

    try {
      if (isAdmin && serverChanged) {
        await run(() =>
          api('/api/v1/settings', {
            method: 'POST',
            body: JSON.stringify({
              workspace_name: normalized.workspace,
              gateway_deployment: normalized.deployment,
              gateway_protocol: normalized.protocol,
              gateway_host: normalized.host,
              gateway_port: normalized.port,
              max_attempts: Number(nextSettings.retries),
              connect_timeout_seconds: Number(nextSettings.connectTimeout),
              first_byte_timeout_seconds: Number(nextSettings.firstByteTimeout),
              stream_idle_timeout_seconds: Number(nextSettings.streamIdleTimeout),
              retry_after_cap_seconds: Number(nextSettings.retryAfterCap),
              log_bodies: nextSettings.logBodies,
              model_normalize_router_prefixes: nextSettings.normalizePrefixes,
              model_normalize_functional_suffixes: nextSettings.normalizeSuffixes,
              model_normalize_explicit_mappings: nextSettings.normalizeMappings,
              ai_pool_enabled: nextSettings.aiPoolEnabled,
            }),
          }), ['settings'],
        )
      }
      setSettings(persisted)
      setDraft(persisted)
      notify(t('settingsSaved'))
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    }
  }

  // The pool switch persists immediately (like the per-service toggles below),
  // so the gateway state can never silently diverge from the shown switch.
  const toggleAiPool = async () => {
    const next = !draft.aiPoolEnabled
    setAiPoolBusy(true)
    try {
      await run(() =>
        api('/api/v1/settings', {
          method: 'POST',
          body: JSON.stringify({ ai_pool_enabled: next }),
        }), ['settings'],
      )
      setDraft((current) => ({ ...current, aiPoolEnabled: next }))
      notify(t('settingsSaved'))
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setAiPoolBusy(false)
    }
  }

  const copy = async (id: number, value: string) => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(id)
      setTimeout(() => setCopied(null), 1500)
    } catch {
      notify(t('clipboardUnavailable'))
    }
  }

  const copyEndpoint = async () => {
    try {
      await navigator.clipboard.writeText(gatewayUrl(draft))
      setEndpointCopied(true)
      setTimeout(() => setEndpointCopied(false), 1500)
      notify(t('endpointCopied'))
    } catch {
      notify(t('clipboardUnavailable'))
    }
  }

  const changeDeployment = (deployment: Settings['deployment']) =>
    setDraft({ ...draft, deployment, protocol: deployment === 'local' ? 'http' : 'https' })

  const exportWorkspace = () => {
    downloadFile(
      'sepia-workspace.json',
      JSON.stringify(
        { settings, providers, routes, plugins, requests, exportedAt: new Date().toISOString() },
        null,
        2,
      ),
    )
    notify(t('workspaceExported'))
  }

  const createKey = async (policy: KeyPolicy) => {
    const created = await run(() =>
      api<{ id: number; plaintext: string }>('/api/v1/keys', {
        method: 'POST',
        body: JSON.stringify(keyInput(policy)),
      }), ['keys'],
    )
    try {
      await run(() =>
        api(`/api/v1/keys/${created.id}/policy`, {
          method: 'POST',
          body: JSON.stringify(policyInput(policy, providers)),
        }), ['keys'],
      )
    } catch (cause) {
      await run(() => api(`/api/v1/keys/${created.id}/revoke`, { method: 'POST' }), ['keys']).catch(
        () => undefined,
      )
      throw cause
    }
    if (!policy.enabled) {
      await run(() =>
        api(`/api/v1/keys/${created.id}/enabled`, {
          method: 'POST',
          body: JSON.stringify({ enabled: false }),
        }), ['keys'],
      )
    }
    setCreatedKey({ ...policy, id: created.id, value: created.plaintext, created: t('justNow') })
    setRevealed(created.id)
    setNewKeyOpen(false)
    notify(t('keyGenerated'))
  }

  const revokeKey = async (id: number) => {
    try {
      await run(() => api(`/api/v1/keys/${id}/revoke`, { method: 'POST' }), ['keys'])
      if (createdKey?.id === id) setCreatedKey(null)
      notify(t('keyRemoved'))
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    }
  }

  const checkUpdate = async () => {
    setUpdatePhase('checking')
    setUpdateError('')
    try {
      setUpdateCheck(await api<UpdateCheckResult>('/api/v1/update/check'))
    } catch (cause) {
      setUpdateError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setUpdatePhase('idle')
    }
  }

  const applyUpdate = async () => {
    if (!updateCheck?.update_available || !updateCheck.platform_supported) return
    const previousVersion = version
    setUpdatePhase('applying')
    setUpdateError('')
    try {
      await api<{ version: string; restarting: boolean }>('/api/v1/update', { method: 'POST' })
      setUpdatePhase('restarting')
      const restarted = await waitForRestart(
        previousVersion,
        async () => (await api<{ version: string }>('/api/v1/version')).version,
      )
      if (restarted) {
        window.location.reload()
        return
      }
      setUpdatePhase('idle')
      setUpdateError(t('updateRestartTimeout'))
    } catch (cause) {
      setUpdatePhase('idle')
      setUpdateError(cause instanceof Error ? cause.message : String(cause))
    }
  }

  const activeTabMeta = tabs.find((item) => item.name === tab)!

  return (
    <div className="settings-view">
      <SectionHeading
        eyebrow={t('homeEyebrow')}
        title={t('settingsTitle')}
        description={t('settingsSubtitle')}
        action={
          <button
            className="button primary"
            type="submit"
            form="settings-form"
            disabled={!dirty}
          >
            <Save size={15} />
            {dirty ? t('saveChanges') : t('allSaved')}
          </button>
        }
      />

      <div className="settings-layout">
        <nav className="settings-nav glass-panel" aria-label={t('settings')}>
          {tabs.map(({ name, icon: Icon, key }) => (
            <button
              type="button"
              key={name}
              className={tab === name ? 'active' : ''}
              onClick={() => setTab(name)}
            >
              <Icon size={16} />
              <span>{t(key)}</span>
              <ChevronRight size={13} />
            </button>
          ))}
          <div className="settings-local-note">
            <ShieldCheck size={17} />
            <strong>{t('browserStorage')}</strong>
            <p>{t('browserStorageDescription')}</p>
          </div>
        </nav>

        <form id="settings-form" className="settings-content" onSubmit={save}>
          <div className="settings-section-heading">
            <h2>{t(activeTabMeta.key)}</h2>
            <p>{t(activeTabMeta.description)}</p>
          </div>

          {tab === 'General' && (
            <>
              <section className="settings-card glass-panel">
                <h3>{t('workspaceSection')}</h3>
                <div className="settings-fields">
                  <SettingsField
                    label={t('workspaceName')}
                    value={draft.workspace}
                    onChange={(v) => update('workspace', v)}
                    disabled={!isAdmin}
                  />
                  <SelectField
                    label={t('language')}
                    value={draft.language}
                    options={['English', '简体中文', '日本語']}
                    onChange={(v) => {
                      update('language', v)
                      setSettings({ ...settings, language: v })
                    }}
                  />
                </div>
                {!isAdmin && <p className="setting-note">{t('settingsAdminOnly')}</p>}
              </section>

              <section className="settings-card glass-panel">
                <h3>{t('preferences')}</h3>
                <SettingRow
                  title={t('reduceMotion')}
                  description={t('reduceMotionDescription')}
                >
                  <Toggle
                    label={t('reduceMotion')}
                    checked={draft.reducedMotion}
                    onChange={() => update('reducedMotion', !draft.reducedMotion)}
                  />
                </SettingRow>
                <SettingRow
                  title={t('compactTables')}
                  description={t('compactTablesDescription')}
                >
                  <Toggle
                    label={t('compactTables')}
                    checked={draft.compact}
                    onChange={() => update('compact', !draft.compact)}
                  />
                </SettingRow>
                <div className="settings-version">
                  <span>{t('frontendVersion')}</span>
                  <span className="mono">
                    {formatVersion(version)} · {t('browserWorkspace')}
                  </span>
                </div>
              </section>

              <section className="settings-card glass-panel">
                <h3>{t('loggingSection')}</h3>
                <fieldset className="settings-readonly-group" disabled={!isAdmin}>
                  <SettingRow
                    title={t('logBodies')}
                    description={t('logBodiesDescription')}
                  >
                    <Toggle
                      checked={draft.logBodies}
                      onChange={() => update('logBodies', !draft.logBodies)}
                      label={t('logBodies')}
                    />
                  </SettingRow>
                </fieldset>
                {!isAdmin && <p className="setting-note">{t('settingsAdminOnly')}</p>}
                <p className="setting-note">{t('logBodiesRetentionNote')}</p>
              </section>

              <section className="settings-card glass-panel">
                <h3>{t('yourData')}</h3>
                <SettingRow
                  title={t('exportWorkspace')}
                  description={t('exportWorkspaceDescription')}
                >
                  <button
                    type="button"
                    className="button secondary"
                    onClick={exportWorkspace}
                  >
                    <Download size={14} />
                    {t('export')}
                  </button>
                </SettingRow>
              </section>
            </>
          )}

          {tab === 'Gateway' && (
            <section className="settings-card glass-panel gateway-network">
              <div className="settings-card-title gateway-network-title">
                <div>
                  <h3>{t('network')}</h3>
                  <p>{t('gatewayDescription')}</p>
                </div>
                <Network size={18} />
              </div>
              <fieldset className="settings-readonly-group" disabled={!isAdmin}>
              <div
                className="gateway-deployment-options"
                role="group"
                aria-label={t('deployment')}
              >
                <button
                  type="button"
                  className={draft.deployment === 'local' ? 'selected' : ''}
                  aria-pressed={draft.deployment === 'local'}
                  onClick={() => changeDeployment('local')}
                >
                  <span className="gateway-deployment-icon">
                    <HardDrive size={19} />
                  </span>
                  <span>
                    <strong>{t('local')}</strong>
                    <small>
                      {t('localDeploymentHint')}
                    </small>
                  </span>
                  {draft.deployment === 'local' && <Check size={15} />}
                </button>
                <button
                  type="button"
                  className={draft.deployment === 'cloud' ? 'selected' : ''}
                  aria-pressed={draft.deployment === 'cloud'}
                  onClick={() => changeDeployment('cloud')}
                >
                  <span className="gateway-deployment-icon">
                    <Cloud size={19} />
                  </span>
                  <span>
                    <strong>{t('cloud')}</strong>
                    <small>
                      {t('cloudDeploymentHint')}
                    </small>
                  </span>
                  {draft.deployment === 'cloud' && <Check size={15} />}
                </button>
              </div>

              <div className="gateway-endpoint">
                <div>
                  <span>{t('gatewayEndpoint')}</span>
                  <code className="mono">{gatewayUrl(draft)}</code>
                  <small>
                    {t('gatewayEndpointHint')}
                  </small>
                </div>
                <button type="button" className="button secondary" onClick={copyEndpoint}>
                  {endpointCopied ? <Check size={14} /> : <Copy size={14} />}
                  {endpointCopied ? t('copied') : t('copyClipboard')}
                </button>
              </div>

              <div className="settings-fields gateway-fields">
                <label className="settings-field">
                  <span>{t('channelProtocol')}</span>
                  {draft.deployment === 'local' ? (
                    <div className="gateway-protocol-static">
                      <ShieldCheck size={14} />
                      HTTP
                      <small>
                        {t('localProtocolHint')}
                      </small>
                    </div>
                  ) : (
                    <div
                      className="gateway-protocol-options"
                      role="group"
                      aria-label={t('channelProtocol')}
                    >
                      <button
                        type="button"
                        className={draft.protocol === 'http' ? 'active' : ''}
                        aria-pressed={draft.protocol === 'http'}
                        onClick={() => update('protocol', 'http')}
                      >
                        HTTP
                      </button>
                      <button
                        type="button"
                        className={draft.protocol === 'https' ? 'active' : ''}
                        aria-pressed={draft.protocol === 'https'}
                        onClick={() => update('protocol', 'https')}
                      >
                        HTTPS
                      </button>
                    </div>
                  )}
                </label>

                <div
                  className={`gateway-address-fields ${
                    draft.deployment === 'cloud' && draft.protocol === 'https'
                      ? 'https-display'
                      : ''
                  }`}
                >
                  <SettingsField
                    label={t('host')}
                    value={draft.host}
                    onChange={(v) => update('host', v)}
                    hint={draft.deployment === 'cloud' ? t('cloudHostHint') : undefined}
                  />
                  <SettingsField
                    label={t('port')}
                    type="number"
                    value={draft.port}
                    onChange={(v) => update('port', v)}
                    required={!(draft.deployment === 'cloud' && draft.protocol === 'https')}
                    hint={
                      draft.deployment === 'cloud' && draft.protocol === 'https'
                        ? t('httpsPortHint')
                        : undefined
                    }
                  />
                </div>
              </div>
              </fieldset>
              {!isAdmin && <p className="setting-note">{t('settingsAdminOnly')}</p>}
            </section>
          )}

          {tab === 'Routing' && (
            <>
              <fieldset className="settings-readonly-group" disabled={!isAdmin}>
                <section className="settings-card glass-panel">
                  <h3>{t('reliability')}</h3>
                  <div className="settings-fields">
                    <SelectField
                      label={t('maximumAttempts')}
                      value={draft.retries}
                      options={Array.from({ length: 16 }, (_, index) => String(index + 1))}
                      onChange={(v) => update('retries', v)}
                    />
                    <SettingsField
                      label={t('connectTimeout')}
                      type="number"
                      value={draft.connectTimeout}
                      onChange={(v) => update('connectTimeout', v)}
                      hint={t('connectTimeoutHint')}
                    />
                    <SettingsField
                      label={t('firstByteTimeout')}
                      type="number"
                      value={draft.firstByteTimeout}
                      onChange={(v) => update('firstByteTimeout', v)}
                      hint={t('firstByteTimeoutHint')}
                    />
                    <SettingsField
                      label={t('streamIdleTimeout')}
                      type="number"
                      value={draft.streamIdleTimeout}
                      onChange={(v) => update('streamIdleTimeout', v)}
                      hint={t('streamIdleTimeoutHint')}
                    />
                    <SettingsField
                      label={t('retryAfterCap')}
                      type="number"
                      value={draft.retryAfterCap}
                      onChange={(v) => update('retryAfterCap', v)}
                      hint={t('retryAfterCapHint')}
                    />
                  </div>
                </section>
                <ModelNormalizeEditor
                  prefixes={draft.normalizePrefixes}
                  suffixes={draft.normalizeSuffixes}
                  mappings={draft.normalizeMappings}
                  onChange={({ prefixes, suffixes, mappings }) =>
                    setDraft({
                      ...draft,
                      normalizePrefixes: prefixes,
                      normalizeSuffixes: suffixes,
                      normalizeMappings: mappings,
                    })
                  }
                />
              </fieldset>
              {!isAdmin && <p className="setting-note">{t('settingsAdminOnly')}</p>}
            </>
          )}

          {tab === 'AI' && (
            <>
              <section className="settings-card glass-panel">
                <h3>{t('aiPoolTitle')}</h3>
                <fieldset className="settings-readonly-group" disabled={!isAdmin || aiPoolBusy}>
                  <SettingRow
                    title={t('aiPoolEnable')}
                    description={t('aiPoolEnableDescription')}
                  >
                    <Toggle
                      checked={draft.aiPoolEnabled}
                      onChange={() => void toggleAiPool()}
                      label={t('aiPoolEnable')}
                    />
                  </SettingRow>
                </fieldset>
                <p className="setting-note">{t('aiPoolDispatchNote')}</p>
              </section>
              {isAdmin && <AiPoolSection />}
              {!isAdmin && <p className="setting-note">{t('aiPoolAdminOnly')}</p>}
            </>
          )}

          {tab === 'Appearance' && (
            <>
              <section className="settings-card glass-panel">
                <h3>{t('theme')}</h3>
                <div className="theme-options">
                  {(
                    [
                      ['Sepia dark', Moon],
                      ['Light', Sun],
                      ['System', Monitor],
                    ] as [string, typeof Moon][]
                  ).map(([name, Icon]) => (
                    <button
                      type="button"
                      key={name}
                      className={`theme-option ${draft.theme === name ? 'selected' : ''}`}
                      onClick={() => update('theme', name)}
                    >
                      <div
                        className={`theme-preview ${
                          name === 'Light'
                            ? 'light-preview'
                            : name === 'System'
                              ? 'system-preview'
                              : 'dark-preview'
                        }`}
                      >
                        <i />
                        <span />
                        <span />
                      </div>
                      <span>
                        <Icon size={14} />
                        {name === 'Sepia dark'
                          ? t('darkTheme')
                          : name === 'Light'
                            ? t('lightTheme')
                            : t('systemTheme')}
                        {draft.theme === name && <Check size={14} />}
                      </span>
                    </button>
                  ))}
                </div>
              </section>

              <section className="settings-card glass-panel">
                <h3>{t('accentColor')}</h3>
                <div className="accent-options">
                  {[
                    ['Sage', '#c4eaaa'],
                    ['Sky', '#a5cfe5'],
                    ['Lavender', '#c3b3e5'],
                    ['Sand', '#dfc49f'],
                  ].map(([name, color]) => (
                    <button
                      type="button"
                      key={name}
                      className={draft.accent === name ? 'selected' : ''}
                      onClick={() => update('accent', name)}
                    >
                      <i style={{ background: color }} />
                      {name === 'Sage'
                        ? t('sage')
                        : name === 'Sky'
                          ? t('sky')
                          : name === 'Lavender'
                            ? t('lavender')
                            : t('sand')}
                      {draft.accent === name && <Check size={13} />}
                    </button>
                  ))}
                </div>
              </section>
            </>
          )}

          {tab === 'Security' && (
            <>
              {isAdmin && <UserManagementSection />}
              <PasskeySection />
              <section className="settings-card glass-panel">
                <div className="settings-card-title">
                  <div>
                    <h3>{t('previewKeys')}</h3>
                    <p>{t('previewKeysDescription')}</p>
                  </div>
                  {isAdmin && (
                    <button
                      type="button"
                      className="button secondary"
                      onClick={() => setNewKeyOpen(true)}
                    >
                      <Plus size={14} />
                      {t('createKey')}
                    </button>
                  )}
                </div>

                {clientKeys.map((key) => {
                  const value = createdKey?.id === key.id ? createdKey.value : ''
                  return (
                    <div className="api-key-row" key={key.id}>
                      <span className="key-icon">
                        <KeyRound size={17} />
                      </span>
                      <div>
                        <strong>{key.name}</strong>
                        <span className="tabular">
                          {key.models.length} {t('models')} · {key.rpm} RPM
                        </span>
                      </div>
                      <code className="mono">
                        {value && revealed === key.id ? value : 'sk-sepia-••••••••'}
                      </code>
                      {value && (
                        <button
                          type="button"
                          className="icon-button"
                          onClick={() => setRevealed(revealed === key.id ? null : key.id)}
                          aria-label={revealed === key.id ? t('hideApiKey') : t('showApiKey')}
                        >
                          {revealed === key.id ? <EyeOff size={15} /> : <Eye size={15} />}
                        </button>
                      )}
                      {value && (
                        <button
                          type="button"
                          className="icon-button"
                          onClick={() => copy(key.id, value)}
                          aria-label={t('copyApiKey')}
                        >
                          {copied === key.id ? <Check size={15} /> : <Copy size={15} />}
                        </button>
                      )}
                      {isAdmin && (
                        <button
                          type="button"
                          className="icon-button danger-icon"
                          onClick={() => revokeKey(key.id)}
                          aria-label={t('deleteNamed', { name: key.name })}
                        >
                          <Trash2 size={15} />
                        </button>
                      )}
                    </div>
                  )
                })}

                {clientKeys.length === 0 && <div className="empty-state">{t('noPreviewKeys')}</div>}
                <div className="settings-inline-note">
                  <ShieldCheck size={14} />
                  {t('keyOneTime')}
                </div>
              </section>

              <section className="settings-card glass-panel">
                <h3>{t('privacy')}</h3>
                <SettingRow
                  title={t('redaction')}
                  description={t('redactionDescription')}
                >
                  <span className="always-on">
                    <Check size={13} />
                    {t('alwaysOn')}
                  </span>
                </SettingRow>
                <SettingRow
                  title={t('credentialHandling')}
                  description={t('credentialHandlingDescription')}
                >
                  <span className="always-on">
                    <Check size={13} />
                    {t('protected')}
                  </span>
                </SettingRow>
              </section>
            </>
          )}

          {tab === 'About' && (
            <>
              <section className="settings-card glass-panel">
                <div className="settings-card-title">
                  <div>
                    <h3>{t('updates')}</h3>
                    <p>{t('updatesDescription')}</p>
                  </div>
                  <button
                    type="button"
                    className="button secondary"
                    disabled={!isAdmin || !updateActionEnabled(updatePhase, updateCheck)}
                    onClick={() => {
                      if (updateCheck?.update_available) void applyUpdate()
                      else void checkUpdate()
                    }}
                  >
                    {updateButtonLabel(updatePhase, updateCheck, {
                      check: t('updates'),
                      checking: t('updateChecking'),
                      apply: t('updateNow'),
                      applying: t('updateApplying'),
                      restarting: t('updateRestarting'),
                    })}
                  </button>
                </div>
                {!isAdmin && <p className="setting-note">{t('updateAdminOnly')}</p>}
                <SettingRow
                  title={t('currentVersion')}
                  description={t('currentVersionDescription')}
                >
                  <code className="mono">{formatVersion(version)}</code>
                </SettingRow>
                {updateCheck && (
                  <SettingRow
                    title={t('latestVersion')}
                    description={
                      updateCheck.published_at
                        ? t('updatePublishedAt', {
                            date: new Date(updateCheck.published_at).toLocaleString(locale),
                          })
                        : t('latestVersionDescription')
                    }
                  >
                    <a
                      className="button text-button"
                      href="https://github.com/lingyuins/Sepia"
                      target="_blank"
                      rel="noreferrer"
                    >
                      <Github size={15} />
                      {t('viewOnGithub')}
                    </a>
                  </SettingRow>
                )}
                {updateCheck && !updateCheck.update_available && (
                  <p className="setting-note">{t('updateUpToDate')}</p>
                )}
                {updateCheck?.update_available && !updateCheck.platform_supported && (
                  <p className="setting-note">{t('updatePlatformUnsupported')}</p>
                )}
                {updatePhase === 'restarting' && (
                  <p className="setting-note">{t('updateRestartingHint')}</p>
                )}
                {updateError && (
                  <p className="setting-note update-error" role="alert">
                    {updateError}
                  </p>
                )}
              </section>

              {updateCheck?.update_available && (
                <section className="settings-card glass-panel">
                  <h3>{t('releaseNotes')}</h3>
                  {updateCheck.release_notes.trim() ? (
                    <pre className="release-notes mono">{updateCheck.release_notes}</pre>
                  ) : (
                    <p className="setting-note">{t('noReleaseNotes')}</p>
                  )}
                  {updateCheck.release_url && (
                    <a
                      className="button text-button"
                      href={updateCheck.release_url}
                      target="_blank"
                      rel="noreferrer"
                    >
                      {t('viewRelease')}
                    </a>
                  )}
                </section>
              )}
            </>
          )}

          <div className="settings-bottom">
            <span>{dirty ? t('unsavedChanges') : t('savedInBrowser')}</span>
            <button
              type="button"
              className="button text-button"
              disabled={!dirty}
              onClick={() => {
                setDraft(settings)
                notify(t('changesDiscarded'))
              }}
            >
              <RotateCcw size={13} />
              {t('discardChanges')}
            </button>
          </div>
        </form>
      </div>

      <KeyPolicyDialog
        open={newKeyOpen}
        onClose={() => setNewKeyOpen(false)}
        onCreate={createKey}
      />
    </div>
  )
}
