import { useEffect, useRef, useState } from 'react'
import { ArrowRight, Box, Check, ChevronDown, Eye, EyeOff, Fingerprint, LoaderCircle, LockKeyhole, Network, RefreshCw, Route, ShieldCheck, TriangleAlert } from 'lucide-react'
import { api, ApiError } from '../api'
import type { Language } from '../i18n'
import { useWorkspace, useTranslation } from '../workspace'
import { authenticationRequest, authenticationResponse, passkeySupported } from '../passkey'

const languages: Language[] = ['English', '简体中文', '日本語']

function Brand() {
  return <span className="brand"><span className="brand-symbol" aria-hidden="true"><i /><i /><i /></span>Sepia<span className="brand-dot">.</span></span>
}

function PasswordField({ id, label, value, onChange, autoComplete, disabled, visible, onToggle }: {
  id: string
  label: string
  value: string
  onChange: (value: string) => void
  autoComplete: string
  disabled: boolean
  visible: boolean
  onToggle: () => void
}) {
  const t = useTranslation()
  return <div className="gateway-setup-field">
    <label htmlFor={id}>{label}</label>
    <div className="gateway-setup-password">
      <input id={id} className="field" type={visible ? 'text' : 'password'} required minLength={8} value={value} onChange={(event) => onChange(event.target.value)} autoComplete={autoComplete} disabled={disabled} />
      <button type="button" className="icon-button" aria-label={visible ? t('gatewayHidePassword') : t('gatewayShowPassword')} title={visible ? t('gatewayHidePassword') : t('gatewayShowPassword')} aria-pressed={visible} onClick={onToggle} disabled={disabled}>{visible ? <EyeOff size={18} /> : <Eye size={18} />}</button>
    </div>
  </div>
}

export function GatewayGate({ children }: { children: React.ReactNode }) {
  const { status, initialization, error, login, loginWithPasskey, checkGateway, settings, setSettings } = useWorkspace()
  const t = useTranslation()
  const [username, setUsername] = useState('admin')
  const [password, setPassword] = useState('')
  const [confirmation, setConfirmation] = useState('')
  const [token, setToken] = useState('')
  const [passwordVisible, setPasswordVisible] = useState(false)
  const [confirmationVisible, setConfirmationVisible] = useState(false)
  const [busy, setBusy] = useState(false)
  const [retrying, setRetrying] = useState(false)
  const [passkeyBusy, setPasskeyBusy] = useState(false)
  const [message, setMessage] = useState('')
  const headingRef = useRef<HTMLHeadingElement>(null)
  const confirmationRef = useRef<HTMLInputElement>(null)
  const phase = status === 'loading' || initialization === null ? 0 : status === 'error' ? 0 : 1
  const screen = `${status}-${initialization}-${phase}`

  useEffect(() => { headingRef.current?.focus(); setMessage('') }, [screen])

  const retry = async () => {
    setRetrying(true)
    setMessage('')
    try { await checkGateway() } catch { /* Workspace exposes the sanitized error. */ }
    finally { setRetrying(false) }
  }

  const recoverInitialization = async (cause: unknown) => {
    try {
      const result = await checkGateway()
      setMessage(result.initialized ? t('gatewayInitializedLogin') : cause instanceof Error ? cause.message : t('gatewayUnavailable'))
    } catch {
      setMessage(cause instanceof ApiError ? cause.message : t('gatewayRetryHint'))
    }
  }

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    setMessage('')
    if (initialization === false && password !== confirmation) {
      confirmationRef.current?.setCustomValidity(t('gatewayPasswordMismatch'))
      confirmationRef.current?.reportValidity()
      return
    }
    confirmationRef.current?.setCustomValidity('')
    setBusy(true)
    const bootstrapping = initialization === false
    let bootstrapCompleted = false
    try {
      if (bootstrapping) {
        await api('/api/v1/auth/bootstrap', { method: 'POST', body: JSON.stringify({ username, password, token }) })
        bootstrapCompleted = true
      }
      await login(username, password)
    } catch (cause) {
      if (bootstrapCompleted || !(cause instanceof ApiError)) {
        await recoverInitialization(cause)
      } else {
        setMessage(cause.message)
      }
    } finally { setBusy(false) }
  }
  const signInWithPasskey = async () => {
    setPasskeyBusy(true)
    setMessage('')
    try {
      const start = await api<{ challenge: string; options: { publicKey: Parameters<typeof authenticationRequest>[0] } }>('/api/v1/auth/passkey/login/options', { method: 'POST', body: '{}' })
      const credential = await navigator.credentials.get({ publicKey: authenticationRequest(start.options.publicKey) })
      if (!credential) throw new Error(t('passkeyCancelled'))
      await loginWithPasskey(authenticationResponse(credential as PublicKeyCredential), start.challenge)
    } catch (cause) {
      if (cause instanceof DOMException && cause.name === 'NotAllowedError') setMessage(t('passkeyCancelled'))
      else setMessage(cause instanceof Error ? cause.message : String(cause))
    } finally { setPasskeyBusy(false) }
  }

  if (status === 'ready') return children

  const initialize = initialization === false
  const credentials = status === 'login' && initialization !== null
  const title = status === 'error' ? 'gatewayConnectionFailed' : status === 'loading' || initialization === null ? 'gatewayChecking' : initialize ? 'gatewayInitializeTitle' : 'gatewayLoginTitle'
  const description = status === 'error' ? 'gatewayConnectionFailedHint' : status === 'loading' || initialization === null ? 'gatewayCheckingHint' : initialize ? 'gatewayInitializeHint' : 'gatewayLoginHint'

  return <main className="gateway-setup">
    <header className="gateway-setup-header">
      <Brand />
      <label className="gateway-setup-language"><span className="sr-only">{t('language')}</span><select aria-label={t('language')} value={settings.language} onChange={(event) => setSettings({ ...settings, language: event.target.value as Language })}>{languages.map((language) => <option key={language} value={language}>{language}</option>)}</select><ChevronDown size={15} aria-hidden="true" /></label>
    </header>
    <div className="gateway-setup-layout">
      <section className="gateway-setup-intro" aria-label={t('gatewaySecurityOverview')}>
        <p className="gateway-setup-eyebrow">{t('gatewayEyebrow')}</p>
        <h2>{t('gatewayIntroTitle')}</h2>
        <p className="gateway-setup-intro-body">{t('gatewayIntroBody')}</p>
        <div className="gateway-setup-route" aria-label={t('gatewayRouteFlow')}>
          <div><span><Box size={21} /></span><strong>{t('gatewayProvider')}</strong></div>
          <i aria-hidden="true" />
          <div><span><Route size={21} /></span><strong>{t('gatewayRoute')}</strong></div>
          <i aria-hidden="true" />
          <div><span><ArrowRight size={21} /></span><strong>{t('gatewayApplication')}</strong></div>
        </div>
        <div className="gateway-setup-security">
          <div><ShieldCheck size={19} /><p><strong>{t('gatewayTokenSecurity')}</strong><span>{t('gatewayTokenSecurityHint')}</span></p></div>
          <div><LockKeyhole size={19} /><p><strong>{t('gatewaySessionSecurity')}</strong><span>{t('gatewaySessionSecurityHint')}</span></p></div>
        </div>
        <p className="gateway-setup-mobile-security"><ShieldCheck size={17} />{t('gatewayMobileSecurity')}</p>
      </section>
      <section className="gateway-setup-panel glass-panel" aria-busy={busy || retrying || status === 'loading'}>
        <ol className="gateway-setup-steps" aria-label={t('gatewaySetupProgress')}>
          {[t('gatewayStepConnect'), t('gatewayStepVerify'), t('gatewayStepConsole')].map((label, index) => <li key={label} className={index < phase ? 'complete' : index === phase ? 'active' : ''} aria-current={index === phase ? 'step' : undefined}><span>{index < phase ? <Check size={13} /> : index + 1}</span><small>{label}</small></li>)}
        </ol>
        <div className="gateway-setup-content">
          <span className={`gateway-setup-state-icon ${status === 'error' ? 'error' : ''}`}>{status === 'error' ? <TriangleAlert size={23} /> : credentials ? initialize ? <ShieldCheck size={23} /> : <LockKeyhole size={23} /> : <Network size={23} />}</span>
          <h1 ref={headingRef} tabIndex={-1}>{t(title)}</h1>
          <p className="gateway-setup-description">{t(description)}</p>
          <div className="gateway-setup-origin"><span>{t('gatewayManagementApi')}</span><code>{window.location.origin}</code><small>{t('gatewaySameOrigin')}</small></div>
          {status === 'error' && <>
            <p className="gateway-setup-error" role="alert">{message || error}</p>
            <button type="button" className="button primary gateway-setup-submit" onClick={() => void retry()} disabled={retrying}>{retrying ? <LoaderCircle className="gateway-setup-spinner" size={17} /> : <RefreshCw size={17} />}{retrying ? t('gatewayRetrying') : t('gatewayRetry')}</button>
          </>}
          {(status === 'loading' || initialization === null) && <p className="gateway-setup-busy" aria-live="polite"><LoaderCircle className="gateway-setup-spinner" size={18} />{t('gatewayChecking')}</p>}
          {credentials && <form onSubmit={submit}>
            <div className="gateway-setup-field"><label htmlFor="gateway-username">{t('gatewayUsername')}</label><input id="gateway-username" className="field" required value={username} onChange={(event) => setUsername(event.target.value)} autoComplete="username" disabled={busy} /></div>
            {initialize && <div className="gateway-setup-field"><label htmlFor="gateway-token">{t('gatewayBootstrapToken')}</label><input id="gateway-token" className="field" required value={token} onChange={(event) => setToken(event.target.value)} autoComplete="off" disabled={busy} /><small>{t('gatewayBootstrapTokenHint')}</small></div>}
            <PasswordField id="gateway-password" label={initialize ? t('gatewayNewPassword') : t('gatewayPassword')} value={password} onChange={(value) => { setPassword(value); confirmationRef.current?.setCustomValidity('') }} autoComplete={initialize ? 'new-password' : 'current-password'} disabled={busy} visible={passwordVisible} onToggle={() => setPasswordVisible((value) => !value)} />
            {initialize && <><div className="gateway-setup-field"><label htmlFor="gateway-confirmation">{t('gatewayConfirmPassword')}</label><input ref={confirmationRef} id="gateway-confirmation" className="field" type={confirmationVisible ? 'text' : 'password'} required minLength={8} value={confirmation} onChange={(event) => { setConfirmation(event.target.value); event.target.setCustomValidity('') }} autoComplete="new-password" disabled={busy} /><small>{t('gatewayPasswordHint')}</small></div><button type="button" className="button text-button gateway-setup-confirm-toggle" aria-label={confirmationVisible ? t('gatewayHidePassword') : t('gatewayShowPassword')} aria-pressed={confirmationVisible} onClick={() => setConfirmationVisible((value) => !value)} disabled={busy}>{confirmationVisible ? <EyeOff size={16} /> : <Eye size={16} />}{confirmationVisible ? t('gatewayHidePassword') : t('gatewayShowPassword')}</button></>}
            {message && <p className="gateway-setup-error" role="alert">{message}</p>}
            <button type="submit" className="button primary gateway-setup-submit" disabled={busy}>{busy ? <LoaderCircle className="gateway-setup-spinner" size={17} /> : initialize ? <ShieldCheck size={17} /> : <LockKeyhole size={17} />}{busy ? t('gatewayAuthenticating') : initialize ? t('gatewayInitialize') : t('gatewaySignIn')}</button>
            <span className="sr-only" aria-live="polite">{busy ? t('gatewayAuthenticating') : ''}</span>
            {!initialize && passkeySupported() && <button type="button" className="button secondary gateway-setup-submit" onClick={() => void signInWithPasskey()} disabled={busy || passkeyBusy}>{passkeyBusy ? <LoaderCircle className="gateway-setup-spinner" size={17} /> : <Fingerprint size={17} />}{passkeyBusy ? t('passkeyWaiting') : t('passkeySignIn')}</button>}
          </form>}
        </div>
      </section>
    </div>
  </main>
}
