import { useEffect, useState } from 'react'
import { LockKeyhole, Network, RefreshCw } from 'lucide-react'
import { api } from '../api'
import { useWorkspace, useTranslation } from '../workspace'

const copy = {
  English: { username: 'Username', password: 'Password', token: 'Bootstrap token', signIn: 'Sign in', initialize: 'Initialize gateway', bootstrapNote: 'Enter the one-time token printed by the gateway on first startup.', retry: 'Retry connection' },
  简体中文: { username: '用户名', password: '密码', token: '初始化令牌', signIn: '登录', initialize: '初始化网关', bootstrapNote: '输入网关首次启动时打印的一次性初始化令牌。', retry: '重试连接' },
  日本語: { username: 'ユーザー名', password: 'パスワード', token: '初期化トークン', signIn: 'サインイン', initialize: 'ゲートウェイを初期化', bootstrapNote: '初回起動時にゲートウェイが表示した一度きりのトークンを入力してください。', retry: '再接続' },
}

export function GatewayGate({ children }: { children: React.ReactNode }) {
  const { status, error, login, settings } = useWorkspace()
  const t = useTranslation()
  const c = copy[settings.language as keyof typeof copy] || copy.English
  const [username, setUsername] = useState('admin')
  const [password, setPassword] = useState('')
  const [token, setToken] = useState('')
  const [initialized, setInitialized] = useState<boolean | null>(null)
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')
  useEffect(() => { if (status === 'ready' || status === 'loading') return; api<{ initialized: boolean }>('/api/v1/auth/status').then((result) => setInitialized(result.initialized)).catch((cause) => setMessage(cause instanceof Error ? cause.message : String(cause))) }, [status])
  if (status === 'ready') return children
  if (status === 'loading') return <main className="gateway-gate"><Network size={30} /><h1>Sepia</h1><p>{t('loading')}</p></main>
  const submit = async (event: React.FormEvent) => {
    event.preventDefault(); setBusy(true); setMessage('')
    try {
      if (initialized === false) await api('/api/v1/auth/bootstrap', { method: 'POST', body: JSON.stringify({ username, password, token }) })
      await login(username, password)
    } catch (cause) { setMessage(cause instanceof Error ? cause.message : String(cause)) }
    finally { setBusy(false) }
  }
  return <main className="gateway-gate"><section className="gateway-gate-card glass-panel"><span className="gateway-gate-icon"><LockKeyhole size={25} /></span><h1>Sepia</h1><p>{error}</p>{initialized !== null ? <form onSubmit={submit}>{initialized === false && <><label className="field-label" htmlFor="gateway-token">{c.token}</label><input id="gateway-token" className="field" required value={token} onChange={(event) => setToken(event.target.value)} autoComplete="off" /><p>{c.bootstrapNote}</p></>}<label className="field-label" htmlFor="gateway-username">{c.username}</label><input id="gateway-username" className="field" required value={username} onChange={(event) => setUsername(event.target.value)} autoComplete="username" /><label className="field-label" htmlFor="gateway-password">{c.password}</label><input id="gateway-password" className="field" required minLength={12} type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete={initialized === false ? 'new-password' : 'current-password'} /><button className="button primary" disabled={busy} type="submit">{busy ? t('loading') : initialized === false ? c.initialize : c.signIn}</button></form> : <button className="button secondary" onClick={() => { setInitialized(null); api<{ initialized: boolean }>('/api/v1/auth/status').then((result) => setInitialized(result.initialized)).catch((cause) => setMessage(cause instanceof Error ? cause.message : String(cause))) }}><RefreshCw size={15} />{c.retry}</button>}{message && <p role="alert">{message}</p>}</section></main>
}
