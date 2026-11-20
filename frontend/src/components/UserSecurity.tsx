import { useEffect, useState } from 'react'
import {
  Fingerprint,
  KeyRound,
  LoaderCircle,
  Plus,
  ShieldCheck,
  Trash2,
  UserRound,
  Users,
} from 'lucide-react'
import { api } from '../api'
import { useWorkspace, useTranslation } from '../workspace'
import type { UserRole } from '../gateway-workspace'
import { passkeySupported, registrationRequest, registrationResponse } from '../passkey'
import { Dropdown } from './Dropdown'
import { DialogActions, Field, Modal } from './UI'

type Passkey = { id: number; name: string; created_at: string; last_used_at: string | null }
type ConsoleUser = { id: number; username: string; role: UserRole; created_at: string }

function formatDate(value: string | null): string {
  if (!value) return '—'
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '—' : date.toLocaleString()
}

export function PasskeySection() {
  const { notify } = useWorkspace()
  const t = useTranslation()
  const [passkeys, setPasskeys] = useState<Passkey[]>([])
  const [busy, setBusy] = useState(false)
  const [name, setName] = useState('')
  const supported = passkeySupported()

  const load = async () => {
    try {
      setPasskeys((await api<{ passkeys: Passkey[] }>('/api/v1/passkeys')).passkeys)
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const addPasskey = async () => {
    setBusy(true)
    try {
      const start = await api<{
        challenge: string
        options: { publicKey: Parameters<typeof registrationRequest>[0] }
      }>('/api/v1/auth/passkey/register/options', { method: 'POST', body: '{}' })
      const credential = await navigator.credentials.create({
        publicKey: registrationRequest(start.options.publicKey),
      })
      if (!credential) throw new Error(t('passkeyCancelled'))
      await api('/api/v1/auth/passkey/register/finish', {
        method: 'POST',
        body: JSON.stringify({
          challenge: start.challenge,
          name: name.trim() || t('passkeyDefaultName'),
          credential: registrationResponse(credential as PublicKeyCredential),
        }),
      })
      setName('')
      await load()
      notify(t('passkeyAdded'))
    } catch (cause) {
      if (cause instanceof DOMException && cause.name === 'NotAllowedError') {
        notify(t('passkeyCancelled'))
      } else {
        notify(cause instanceof Error ? cause.message : String(cause))
      }
    } finally {
      setBusy(false)
    }
  }

  const removePasskey = async (id: number) => {
    try {
      await api(`/api/v1/passkeys/${id}`, { method: 'DELETE' })
      await load()
      notify(t('passkeyRemoved'))
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    }
  }

  return (
    <section className="settings-card glass-panel">
      <div className="settings-card-title">
        <div>
          <h3>{t('passkeys')}</h3>
          <p>{t('passkeysDescription')}</p>
        </div>
        <Fingerprint size={18} />
      </div>
      {!supported && <p className="setting-note">{t('passkeyUnsupported')}</p>}
      {supported && (
        <div className="passkey-add-row">
          <input
            className="field"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={t('passkeyNamePlaceholder')}
            aria-label={t('passkeyNamePlaceholder')}
            disabled={busy}
          />
          <button
            type="button"
            className="button secondary"
            onClick={() => void addPasskey()}
            disabled={busy}
          >
            {busy ? <LoaderCircle className="gateway-setup-spinner" size={14} /> : <Plus size={14} />}
            {busy ? t('passkeyWaiting') : t('passkeyAdd')}
          </button>
        </div>
      )}
      {passkeys.map((passkey) => (
        <div className="api-key-row" key={passkey.id}>
          <span className="key-icon">
            <KeyRound size={17} />
          </span>
          <div>
            <strong>{passkey.name}</strong>
            <span>
              {t('passkeyCreatedAt', { date: formatDate(passkey.created_at) })} ·{' '}
              {t('passkeyLastUsed', { date: formatDate(passkey.last_used_at) })}
            </span>
          </div>
          <button
            type="button"
            className="icon-button danger-icon"
            onClick={() => void removePasskey(passkey.id)}
            aria-label={t('deleteNamed', { name: passkey.name })}
          >
            <Trash2 size={15} />
          </button>
        </div>
      ))}
      {passkeys.length === 0 && <div className="empty-state">{t('noPasskeys')}</div>}
    </section>
  )
}

export function UserManagementSection() {
  const { session, notify, checkGateway, logout } = useWorkspace()
  const t = useTranslation()
  const [users, setUsers] = useState<ConsoleUser[]>([])
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [role, setRole] = useState<UserRole>('viewer')
  const [busy, setBusy] = useState(false)

  const [resetTarget, setResetTarget] = useState<ConsoleUser | null>(null)
  const [resetValue, setResetValue] = useState('')
  const [resetBusy, setResetBusy] = useState(false)

  const [deleteTarget, setDeleteTarget] = useState<ConsoleUser | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)

  const roleOptions = (['admin', 'editor', 'viewer'] as UserRole[]).map((value) => ({
    value,
    label: t(`role_${value}`),
  }))

  const load = async () => {
    try {
      setUsers((await api<{ users: ConsoleUser[] }>('/api/v1/users')).users)
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const createUser = async () => {
    if (!username.trim() || password.length < 8) {
      notify(t('userPasswordHint'))
      return
    }
    setBusy(true)
    try {
      await api('/api/v1/users', {
        method: 'POST',
        body: JSON.stringify({ username: username.trim(), password, role }),
      })
      setUsername('')
      setPassword('')
      setRole('viewer')
      await load()
      notify(t('userCreated'))
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }

  const changeRole = async (user: ConsoleUser, next: UserRole) => {
    try {
      await api(`/api/v1/users/${user.id}/role`, {
        method: 'POST',
        body: JSON.stringify({ role: next }),
      })
      await load()
      if (session?.username === user.username) await checkGateway()
      notify(t('userRoleUpdated'))
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    }
  }

  const confirmResetPassword = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!resetTarget) return
    if (resetValue.length < 8) {
      notify(t('userPasswordHint'))
      return
    }
    setResetBusy(true)
    try {
      await api(`/api/v1/users/${resetTarget.id}/password`, {
        method: 'POST',
        body: JSON.stringify({ password: resetValue }),
      })
      setResetTarget(null)
      setResetValue('')
      notify(t('userPasswordUpdated'))
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setResetBusy(false)
    }
  }

  const confirmRemoveUser = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!deleteTarget) return
    setDeleteBusy(true)
    try {
      await api(`/api/v1/users/${deleteTarget.id}`, { method: 'DELETE' })
      if (session?.username === deleteTarget.username) {
        setDeleteTarget(null)
        await logout()
        return
      }
      setDeleteTarget(null)
      await load()
      notify(t('userRemoved'))
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setDeleteBusy(false)
    }
  }

  return (
    <section className="settings-card glass-panel">
      <div className="settings-card-title">
        <div>
          <h3>{t('consoleUsers')}</h3>
          <p>{t('consoleUsersDescription')}</p>
        </div>
        <Users size={18} />
      </div>

      {users.map((user) => (
        <div className="api-key-row user-row" key={user.id}>
          <span className="key-icon">
            <UserRound size={17} />
          </span>
          <div>
            <strong>
              {user.username}
              {session?.username === user.username && (
                <span className="user-self-marker"> · {t('you')}</span>
              )}
            </strong>
            <span>{t('userCreatedAt', { date: formatDate(user.created_at) })}</span>
          </div>
          <Dropdown
            label={t('role')}
            value={user.role}
            options={roleOptions}
            onChange={(value) => void changeRole(user, value as UserRole)}
            variant="compact"
          />
          <button
            type="button"
            className="icon-button"
            onClick={() => {
              setResetTarget(user)
              setResetValue('')
            }}
            aria-label={t('userResetPassword')}
            title={t('userResetPassword')}
          >
            <KeyRound size={15} />
          </button>
          <button
            type="button"
            className="icon-button danger-icon"
            onClick={() => setDeleteTarget(user)}
            aria-label={t('deleteNamed', { name: user.username })}
          >
            <Trash2 size={15} />
          </button>
        </div>
      ))}

      <div className="user-create-form">
        <input
          className="field"
          value={username}
          onChange={(event) => setUsername(event.target.value)}
          placeholder={t('gatewayUsername')}
          aria-label={t('gatewayUsername')}
          required
          autoComplete="off"
          disabled={busy}
        />
        <input
          className="field"
          type="password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          placeholder={t('gatewayNewPassword')}
          aria-label={t('gatewayNewPassword')}
          required
          minLength={8}
          autoComplete="new-password"
          disabled={busy}
        />
        <Dropdown
          label={t('role')}
          value={role}
          options={roleOptions}
          onChange={(value) => setRole(value as UserRole)}
          variant="compact"
        />
        <button
          type="button"
          className="button secondary"
          onClick={() => void createUser()}
          disabled={busy}
        >
          {busy ? <LoaderCircle className="gateway-setup-spinner" size={14} /> : <Plus size={14} />}
          {t('userCreate')}
        </button>
      </div>

      <p className="setting-note">
        <ShieldCheck size={14} />
        {t('userPasswordHint')}
      </p>

      <Modal
        open={!!resetTarget}
        onClose={() => {
          setResetTarget(null)
          setResetValue('')
        }}
        title={t('userResetPassword')}
        description={
          resetTarget ? `${resetTarget.username} · ${t('userResetPasswordPrompt')}` : undefined
        }
      >
        <form onSubmit={(event) => void confirmResetPassword(event)}>
          <Field
            label={t('gatewayNewPassword')}
            type="password"
            value={resetValue}
            onChange={setResetValue}
            placeholder={t('gatewayNewPassword')}
            required
            minLength={8}
            autoComplete="new-password"
            hint={t('userPasswordHint')}
          />
          <DialogActions
            onClose={() => {
              setResetTarget(null)
              setResetValue('')
            }}
            label={t('saveChanges')}
            disabled={resetBusy || resetValue.length < 8}
          />
        </form>
      </Modal>

      <Modal
        open={!!deleteTarget}
        onClose={() => setDeleteTarget(null)}
        title={deleteTarget ? t('deleteNamed', { name: deleteTarget.username }) : ''}
        description={
          deleteTarget ? t('userDeleteConfirm', { name: deleteTarget.username }) : undefined
        }
      >
        <form onSubmit={(event) => void confirmRemoveUser(event)}>
          <DialogActions
            onClose={() => setDeleteTarget(null)}
            label={t('deleteNamed', { name: deleteTarget?.username || '' })}
            disabled={deleteBusy}
            danger
          />
        </form>
      </Modal>
    </section>
  )
}