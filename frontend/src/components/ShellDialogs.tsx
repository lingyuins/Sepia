import { useState } from 'react'
import {
  Activity,
  Code2,
  Download,
  ExternalLink,
  KeyRound,
  Layers3,
  LogOut,
  Route,
  Settings,
  ShieldCheck,
} from 'lucide-react'
import { downloadFile, useWorkspace, useTranslation } from '../workspace'
import { gatewayUrl } from '../gateway'
import { Modal, StatusText } from './UI'
import { CopyButton } from './Provider'

const HELP_TOPICS = ['Quick start', 'Core concepts', 'Protocols', 'Keyboard shortcuts'] as const

export function HelpDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { settings, notify, routes } = useWorkspace()
  const t = useTranslation()
  const [topic, setTopic] = useState<(typeof HELP_TOPICS)[number]>('Quick start')

  const endpoint = gatewayUrl(settings)
  const origin = gatewayUrl(settings, '').replace(/\/+$/, '')
  const exampleModel = routes.find((route) => route.active)?.slug || 'your-route'
  const code = `curl ${endpoint}/chat/completions \\\n  -H "Authorization: Bearer YOUR_CLIENT_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{"model":"${exampleModel}","messages":[{"role":"user","content":"Hello, Sepia."}]}'`

  const download = () => {
    downloadFile('sepia-quick-start.txt', code, 'text/plain')
    notify(t('quickstartDownloaded'))
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t('helpTitle')}
      description={t('helpSubtitle')}
      size="help-dialog"
    >
      <div className="help-layout">
        <nav aria-label={t('helpTitle')}>
          {HELP_TOPICS.map((item) => (
            <button
              type="button"
              key={item}
              className={topic === item ? 'active' : ''}
              onClick={() => setTopic(item)}
            >
              {item === 'Quick start'
                ? t('quickStart')
                : item === 'Core concepts'
                  ? t('coreConcepts')
                  : item === 'Protocols'
                    ? t('protocols')
                    : t('shortcuts')}
            </button>
          ))}
        </nav>

        <div className="help-content">
          {topic === 'Quick start' && (
            <>
              <span className="eyebrow">{t('firstRequest')}</span>
              <h3>{t('oneEndpoint')}</h3>
              <p>{t('quickStartDescription')}</p>
              <div className="help-code">
                <code className="mono">{code}</code>
                <CopyButton value={code} onCopy={() => notify(t('exampleCopied'))} />
              </div>
              <div className="help-note">
                <ShieldCheck size={15} />
                {t('exampleNote')}
              </div>
              <button
                type="button"
                className="button secondary help-download"
                onClick={download}
              >
                <Download size={14} />
                {t('downloadQuickstart')}
              </button>
              <p className="help-download-hint">{t('helpDownloadHint')}</p>
            </>
          )}

          {topic === 'Core concepts' && (
            <>
              <span className="eyebrow">{t('footer')}</span>
              <h3>{t('conceptsTitle')}</h3>
              <div className="concept-row">
                <Layers3 size={18} />
                <div>
                  <strong>{t('providers')}</strong>
                  <p>{t('providersConcept')}</p>
                </div>
              </div>
              <div className="concept-row">
                <Route size={18} />
                <div>
                  <strong>{t('routing')}</strong>
                  <p>{t('routesConcept')}</p>
                </div>
              </div>
              <div className="concept-row">
                <KeyRound size={18} />
                <div>
                  <strong>{t('previewKeys')}</strong>
                  <p>{t('keysConcept')}</p>
                </div>
              </div>
              <div className="concept-row">
                <Activity size={18} />
                <div>
                  <strong>{t('requests')}</strong>
                  <p>{t('requestsConcept')}</p>
                </div>
              </div>
              <div className="concept-row">
                <Code2 size={18} />
                <div>
                  <strong>{t('plugins')}</strong>
                  <p>{t('pluginsConcept')}</p>
                </div>
              </div>
            </>
          )}

          {topic === 'Protocols' && (
            <>
              <span className="eyebrow">{t('protocols')}</span>
              <h3>{t('protocolsTitle')}</h3>
              <p>{t('protocolsDescription')}</p>
              <div className="protocol-row">
                <code className="mono">POST {endpoint}/chat/completions</code>
                <code className="mono">POST {endpoint}/responses</code>
                <p>{t('protocolOpenAI')}</p>
              </div>
              <div className="protocol-row">
                <code className="mono">POST {endpoint}/messages</code>
                <p>{t('protocolAnthropic')}</p>
              </div>
              <div className="protocol-row">
                <code className="mono">
                  POST {endpoint}/models/&lt;model&gt;:generateContent
                </code>
                <p>{t('protocolGemini')}</p>
              </div>
              <div className="protocol-row">
                <code className="mono">POST {endpoint}/embeddings</code>
                <code className="mono">POST {endpoint}/rerank</code>
                <code className="mono">POST {endpoint}/moderations</code>
                <code className="mono">POST {endpoint}/images/generations</code>
                <code className="mono">POST {endpoint}/images/edits</code>
                <code className="mono">POST {endpoint}/audio/speech</code>
                <code className="mono">POST {endpoint}/audio/transcriptions</code>
                <code className="mono">POST {endpoint}/audio/translations</code>
                <code className="mono">POST {endpoint}/search</code>
                <code className="mono">POST {endpoint}/alpha/search</code>
                <code className="mono">POST {endpoint}/tools/search</code>
                <code className="mono">POST {endpoint}/tools/fetch</code>
                <code className="mono">POST {endpoint}/videos</code>
                <code className="mono">GET {endpoint}/videos/&lt;task_id&gt;</code>
                <code className="mono">POST {endpoint}/video/generations</code>
                <code className="mono">GET {endpoint}/video/generations/&lt;task_id&gt;</code>
                <p>{t('protocolAuxiliary')}</p>
              </div>
              <div className="protocol-row">
                <code className="mono">POST {origin}/suno/submit/&lt;action&gt;</code>
                <code className="mono">POST {origin}/suno/fetch</code>
                <code className="mono">GET {origin}/suno/fetch/&lt;task_id&gt;</code>
                <p>{t('protocolSuno')}</p>
              </div>
              <div className="help-note">
                <ShieldCheck size={15} />
                {t('protocolModelsNote')}
              </div>
            </>
          )}

          {topic === 'Keyboard shortcuts' && (
            <>
              <span className="eyebrow">{t('fasterKeyboard')}</span>
              <h3>{t('keyboardTitle')}</h3>
              <div className="shortcut-row">
                <span>{t('searchWorkspace')}</span>
                <kbd>Ctrl / ⌘ K</kbd>
              </div>
              <div className="shortcut-row">
                <span>{t('closeDialog')}</span>
                <kbd>Esc</kbd>
              </div>
              <div className="shortcut-row">
                <span>{t('moveFields')}</span>
                <kbd>Tab</kbd>
              </div>
              <div className="shortcut-row">
                <span>{t('submitForm')}</span>
                <kbd>Enter</kbd>
              </div>
            </>
          )}
        </div>
      </div>
    </Modal>
  )
}

export function WorkspaceDialog({
  open,
  onClose,
  onSettings,
}: {
  open: boolean
  onClose: () => void
  onSettings: () => void
}) {
  const { settings, notify, logout, session } = useWorkspace()
  const t = useTranslation()

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={settings.workspace}
      description={t('yourWorkspace')}
    >
      <div className="workspace-info">
        <span className="workspace-avatar">
          {(session?.username || settings.workspace).charAt(0).toUpperCase()}
        </span>
        <div>
          <strong>{session?.username || settings.workspace}</strong>
          <p>
            {session ? t(`role_${session.role}`) : t('administrator')} · {t('browserWorkspace')}
          </p>
        </div>
        <StatusText tone="positive">{t('active')}</StatusText>
      </div>

      <div className="workspace-menu">
        <button
          type="button"
          onClick={() => {
            onSettings()
            onClose()
          }}
        >
          <Settings size={16} className="settings-icon" />
          {t('workspaceSettings')}
          <span>{t('settingsMenuHint')}</span>
        </button>

        <button
          type="button"
          onClick={() => {
            const element = document.createElement('a')
            element.href = gatewayUrl(settings, '/v1/models')
            element.target = '_blank'
            element.rel = 'noreferrer'
            element.click()
            notify(t('gatewayOpened'))
          }}
        >
          <ExternalLink size={16} />
          {t('openGateway')}
          <span className="mono">
            {settings.host}:{settings.port}
          </span>
        </button>

        <button
          type="button"
          onClick={() => {
            logout().catch((cause) =>
              notify(cause instanceof Error ? cause.message : String(cause)),
            )
            onClose()
          }}
        >
          <LogOut size={16} />
          {t('logout')}
          <span>{t('logoutHint')}</span>
        </button>
      </div>
    </Modal>
  )
}
