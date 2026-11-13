import { useEffect, useRef, useState } from 'react'
import { ArrowLeft, ArrowRight, Check, ChevronDown, Cloud, HardDrive, KeyRound, Layers3, Network, Route, ShieldCheck } from 'lucide-react'
import { completeOnboarding, type OnboardingDraft } from '../onboarding'
import { gatewayUrl } from '../gateway'
import { useWorkspace } from '../workspace'
import type { Language } from '../i18n'

const languages: { value: Language; label: string; short: string }[] = [
  { value: 'English', label: 'English', short: 'EN' },
  { value: '简体中文', label: '简体中文', short: '中' },
  { value: '日本語', label: '日本語', short: '日' },
]

const copy = {
  English: {
    eyebrow: 'A quieter way to route your models',
    heroTitle: 'One workspace.\nEvery model, your way.',
    heroBody: 'Bring providers, routing rules, and request visibility together. Set up your gateway workspace in a few simple steps.',
    localFirst: 'Your workspace, your choices',
    localFirstBody: 'Gateway configuration is stored by the gateway; interface preferences stay in this browser.',
    providers: 'Providers', routing: 'Routing', application: 'Your application',
    setup: 'Workspace setup', step: 'Step {step} of 3', preview: 'Gateway workspace',
    steps: ['Workspace', 'Gateway', 'Ready'],
    workspaceTitle: 'Make this workspace yours', workspaceBody: 'Choose a name and language. You can change both later in Settings.',
    workspaceName: 'Workspace name', workspaceHint: 'For your own projects or a small team.', language: 'Language',
    gatewayTitle: 'Where will your gateway live?', gatewayBody: 'Set the endpoint your applications will use. This saves an address; it does not start or connect to a service.',
    local: 'Local gateway', localDescription: 'An endpoint on your machine or network', cloud: 'Cloud gateway', cloudDescription: 'A gateway reachable over the internet',
    protocol: 'Protocol', host: 'Host', port: 'Port', localProtocolHint: 'Local endpoints use HTTP',
    readyTitle: 'Your routing workspace is ready', readyBody: 'Next, authenticate with the gateway and add providers and routing rules.',
    workspaceLabel: 'Workspace', endpointLabel: 'Gateway endpoint', previewNote: 'The gateway will ask for administrator credentials, or a one-time bootstrap token on first startup.',
    continue: 'Continue', back: 'Back', finish: 'Enter workspace', skip: 'Skip workspace preferences',
    required: 'Enter a workspace name to continue.', invalidGateway: 'Enter a valid host and a port between 1 and 65535.',
  },
  简体中文: {
    eyebrow: '让模型路由更清晰',
    heroTitle: '一个工作区，\n连接你的每个模型。',
    heroBody: '将供应商、路由规则和请求观测集中在一起，用几个简单步骤准备好你的 AI 网关工作区。',
    localFirst: '工作区由你掌握', localFirstBody: '网关配置由网关持久化，界面偏好保存在当前浏览器中。',
    providers: '模型供应商', routing: '智能路由', application: '你的应用',
    setup: '工作区初始化', step: '第 {step} 步，共 3 步', preview: '网关工作区',
    steps: ['工作区', '网关', '完成'],
    workspaceTitle: '先从你的工作区开始', workspaceBody: '设置名称和语言，之后可以随时在设置中修改。',
    workspaceName: '工作区名称', workspaceHint: '适合个人项目或小团队使用。', language: '界面语言',
    gatewayTitle: '你的网关部署在哪里？', gatewayBody: '设置应用将使用的网关地址。这里仅保存地址，不会启动或连接真实服务。',
    local: '本地网关', localDescription: '部署在本机或局域网中的网关', cloud: '云端网关', cloudDescription: '可通过公网访问的网关',
    protocol: '协议', host: '主机地址', port: '端口', localProtocolHint: '本地网关使用 HTTP',
    readyTitle: '路由工作区已准备好', readyBody: '接下来登录网关，添加供应商并配置路由规则。',
    workspaceLabel: '工作区', endpointLabel: '网关地址', previewNote: '网关会要求管理员登录；首次启动时需要输入一次性初始化令牌。',
    continue: '继续', back: '上一步', finish: '进入工作区', skip: '跳过工作区偏好设置',
    required: '请输入工作区名称后继续。', invalidGateway: '请输入有效的主机地址和 1–65535 之间的端口。',
  },
  日本語: {
    eyebrow: 'モデルルーティングを、もっと明快に',
    heroTitle: 'ひとつのワークスペース。\nすべてのモデルを、あなたの方法で。',
    heroBody: 'プロバイダー、ルーティングルール、リクエストの可視化をひとつに。簡単な設定でゲートウェイを始められます。',
    localFirst: 'ワークスペースはあなたのもの', localFirstBody: 'ゲートウェイ設定はゲートウェイに保存され、表示設定はこのブラウザに保存されます。',
    providers: 'プロバイダー', routing: 'ルーティング', application: 'アプリケーション',
    setup: 'ワークスペース設定', step: '3 ステップ中 {step}', preview: 'ゲートウェイワークスペース',
    steps: ['ワークスペース', 'ゲートウェイ', '完了'],
    workspaceTitle: 'ワークスペースを作成', workspaceBody: '名前と言語を選択してください。後から設定で変更できます。',
    workspaceName: 'ワークスペース名', workspaceHint: '個人プロジェクトや小さなチームに。', language: '表示言語',
    gatewayTitle: 'ゲートウェイの場所', gatewayBody: 'アプリケーションが使用するアドレスを保存します。サービスの起動や接続は行いません。',
    local: 'ローカルゲートウェイ', localDescription: '自分のマシンまたはネットワーク内', cloud: 'クラウドゲートウェイ', cloudDescription: 'インターネットからアクセス可能',
    protocol: 'プロトコル', host: 'ホスト', port: 'ポート', localProtocolHint: 'ローカルでは HTTP を使用',
    readyTitle: 'ルーティングの準備ができました', readyBody: 'ゲートウェイに認証し、プロバイダーとルーティングルールを設定します。',
    workspaceLabel: 'ワークスペース', endpointLabel: 'ゲートウェイアドレス', previewNote: '管理者認証が必要です。初回起動時は一度きりの初期化トークンを使用します。',
    continue: '続ける', back: '戻る', finish: 'ワークスペースへ', skip: '表示設定をスキップ',
    required: 'ワークスペース名を入力してください。', invalidGateway: '有効なホストと 1–65535 のポートを入力してください。',
  },
} as const

export default function Onboarding({ onComplete }: { onComplete: () => void }) {
  const { settings, setSettings } = useWorkspace()
  const [draft, setDraft] = useState<OnboardingDraft>({ workspace: settings.workspace, language: settings.language as Language, deployment: settings.deployment, protocol: settings.protocol, host: settings.host, port: settings.port })
  const [step, setStep] = useState(1)
  const [error, setError] = useState('')
  const headingRef = useRef<HTMLHeadingElement>(null)
  const t = copy[draft.language]
  const update = <K extends keyof OnboardingDraft>(key: K, value: OnboardingDraft[K]) => { setDraft((current) => ({ ...current, [key]: value })); setError('') }

  useEffect(() => { document.documentElement.lang = draft.language === '简体中文' ? 'zh-CN' : draft.language === '日本語' ? 'ja' : 'en' }, [draft.language])
  useEffect(() => { headingRef.current?.focus() }, [step])

  const next = () => {
    if (step === 1 && !draft.workspace.trim()) { setError(t.required); return }
    if (step === 2) {
      const port = Number(draft.port)
      try { gatewayUrl(draft) } catch { setError(t.invalidGateway); return }
      if (!draft.host.trim() || !Number.isInteger(port) || port < 1 || port > 65535) { setError(t.invalidGateway); return }
    }
    setError(''); setStep((current) => Math.min(3, current + 1))
  }
  const finish = () => { setSettings(completeOnboarding(settings, draft)); onComplete() }
  const skip = () => onComplete()
  const endpoint = (() => { try { return gatewayUrl(draft) } catch { return `${draft.host}:${draft.port}/v1` } })()

  return <main className="onboarding">
    <header className="onboarding-header"><div className="brand"><span className="brand-symbol"><i /><i /><i /></span><span>sepia<span className="brand-dot">.</span></span></div><span className="onboarding-preview">{t.preview}</span></header>
    <div className="onboarding-layout">
      <section className="onboarding-intro">
        <p className="onboarding-eyebrow">{t.eyebrow}</p>
        <h1>{t.heroTitle}</h1>
        <p className="onboarding-intro-body">{t.heroBody}</p>
        <div className="onboarding-route" aria-label={`${t.providers} → ${t.routing} → ${t.application}`}>
          <div><span><Layers3 size={18} /></span><strong>{t.providers}</strong></div><i /><div className="onboarding-route-core"><span><Route size={20} /></span><strong>{t.routing}</strong><small>sepia</small></div><i /><div><span><Network size={18} /></span><strong>{t.application}</strong></div>
        </div>
        <div className="onboarding-local-note"><ShieldCheck size={20} /><div><strong>{t.localFirst}</strong><p>{t.localFirstBody}</p></div></div>
      </section>
      <section className="onboarding-panel glass-panel" aria-label={t.setup}>
        <div className="onboarding-panel-heading"><div><span>{t.setup}</span><small>{t.step.replace('{step}', String(step))}</small></div><ol>{t.steps.map((label, index) => <li key={label} className={step === index + 1 ? 'active' : step > index + 1 ? 'complete' : ''} aria-current={step === index + 1 ? 'step' : undefined}><span>{step > index + 1 ? <Check size={12} /> : index + 1}</span><small>{label}</small></li>)}</ol></div>
        <form onSubmit={(event) => { event.preventDefault(); step === 3 ? finish() : next() }}>
          <div className="onboarding-form-content" key={step}>
            <h2 ref={headingRef} tabIndex={-1}>{step === 1 ? t.workspaceTitle : step === 2 ? t.gatewayTitle : t.readyTitle}</h2>
            <p className="onboarding-step-description">{step === 1 ? t.workspaceBody : step === 2 ? t.gatewayBody : t.readyBody}</p>
            {step === 1 && <div className="onboarding-fields"><label className="onboarding-field"><span>{t.workspaceName}</span><input autoFocus value={draft.workspace} onChange={(event) => update('workspace', event.target.value)} aria-describedby="workspace-hint" maxLength={80} /><small id="workspace-hint">{t.workspaceHint}</small></label><label className="onboarding-field"><span>{t.language}</span><div className="onboarding-select"><select value={draft.language} onChange={(event) => update('language', event.target.value as Language)}>{languages.map((language) => <option key={language.value} value={language.value}>{language.label}</option>)}</select><ChevronDown size={16} /></div></label></div>}
            {step === 2 && <><div className="onboarding-deployment" role="group" aria-label={t.gatewayTitle}><button type="button" className={draft.deployment === 'local' ? 'selected' : ''} aria-pressed={draft.deployment === 'local'} onClick={() => { update('deployment', 'local'); update('protocol', 'http') }}><HardDrive size={20} /><span><strong>{t.local}</strong><small>{t.localDescription}</small></span>{draft.deployment === 'local' && <Check size={16} />}</button><button type="button" className={draft.deployment === 'cloud' ? 'selected' : ''} aria-pressed={draft.deployment === 'cloud'} onClick={() => update('deployment', 'cloud')}><Cloud size={20} /><span><strong>{t.cloud}</strong><small>{t.cloudDescription}</small></span>{draft.deployment === 'cloud' && <Check size={16} />}</button></div><div className="onboarding-fields gateway-onboarding-fields"><label className="onboarding-field"><span>{t.protocol}</span><div className="onboarding-select"><select value={draft.protocol} disabled={draft.deployment === 'local'} onChange={(event) => update('protocol', event.target.value as 'http' | 'https')}><option value="http">HTTP</option><option value="https">HTTPS</option></select><ChevronDown size={16} /></div>{draft.deployment === 'local' && <small>{t.localProtocolHint}</small>}</label><label className="onboarding-field onboarding-host"><span>{t.host}</span><input value={draft.host} onChange={(event) => update('host', event.target.value)} spellCheck={false} /></label><label className="onboarding-field"><span>{t.port}</span><input type="number" min="1" max="65535" value={draft.port} onChange={(event) => update('port', event.target.value)} /></label></div></>}
            {step === 3 && <div className="onboarding-summary"><div><span><KeyRound size={17} />{t.workspaceLabel}</span><strong>{draft.workspace.trim()}</strong></div><div><span><Network size={17} />{t.endpointLabel}</span><code>{endpoint}</code></div><p><ShieldCheck size={16} />{t.previewNote}</p></div>}
            {error && <p className="onboarding-error" role="alert">{error}</p>}
          </div>
          <div className="onboarding-actions">{step > 1 ? <button type="button" className="button secondary" onClick={() => { setError(''); setStep(step - 1) }}><ArrowLeft size={15} />{t.back}</button> : <span />}<button className="button primary" type="submit">{step === 3 ? t.finish : t.continue}<ArrowRight size={15} /></button></div>
        </form>
        <button type="button" className="onboarding-skip" onClick={skip}>{t.skip}</button>
      </section>
    </div>
    <footer className="onboarding-footer">sepia <span>·</span> Thoughtfully routed. Effortlessly yours.</footer>
  </main>
}
