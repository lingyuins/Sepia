import type { ReactNode } from 'react'
import { Dialog } from '@headlessui/react'
import { X, type LucideIcon } from 'lucide-react'
import { useWorkspace, useTranslation } from '../workspace'
import { ProviderMark } from './Provider'

export function SectionHeading({
  eyebrow,
  title,
  description,
  action,
}: {
  eyebrow?: string
  title: string
  description: string
  action?: ReactNode
}) {
  return (
    <div className="page-heading">
      <div className="page-heading-copy">
        {eyebrow && <div className="eyebrow">{eyebrow}</div>}
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {action && <div className="page-heading-actions">{action}</div>}
    </div>
  )
}

export function MetricCard({
  title,
  value,
  foot,
  icon: Icon,
}: {
  title: string
  value: string
  foot: string
  icon: LucideIcon
}) {
  return (
    <div className="metric-card">
      <div className="metric-top">
        <span>{title}</span>
        <span className="metric-icon" aria-hidden="true">
          <Icon size={15} />
        </span>
      </div>
      <div className="metric-value tabular">{value}</div>
      <div className="metric-foot">{foot}</div>
    </div>
  )
}

export function StatusDot() {
  return <span className="status-dot" aria-hidden="true" />
}

export function StatusText({
  tone = 'neutral',
  children,
}: {
  tone?: 'positive' | 'warning' | 'danger' | 'neutral'
  children: ReactNode
}) {
  return <span className={`status-text tone-${tone}`}>{children}</span>
}

export function ProviderCell({ name }: { name: string }) {
  const { providers } = useWorkspace()
  const provider = providers.find((item) => item.name === name)
  return (
    <span className="provider-cell">
      {provider ? (
        <ProviderMark {...provider} small />
      ) : (
        <span className="provider-mark small">{name.charAt(0)}</span>
      )}
      <span className="provider-cell-name">{name}</span>
    </span>
  )
}

export function Modal({
  open,
  onClose,
  title,
  description,
  children,
  size = '',
}: {
  open: boolean
  onClose: () => void
  title: string
  description?: string
  children: ReactNode
  size?: string
}) {
  const t = useTranslation()
  return (
    <Dialog open={open} onClose={onClose} className="dialog-root">
      <div className="dialog-backdrop" aria-hidden="true" />
      <div className="dialog-position">
        <Dialog.Panel className={`dialog-panel glass-panel ${size}`}>
          <button
            type="button"
            className="icon-button dialog-close"
            onClick={onClose}
            aria-label={t('close')}
          >
            <X size={18} />
          </button>
          <Dialog.Title>{title}</Dialog.Title>
          {description && <Dialog.Description>{description}</Dialog.Description>}
          {children}
        </Dialog.Panel>
      </div>
    </Dialog>
  )
}

export function Field({
  label,
  value,
  onChange,
  placeholder = '',
  type = 'text',
  required = false,
  hint,
  minLength,
  autoComplete,
}: {
  label: string
  value: string
  onChange: (value: string) => void
  placeholder?: string
  type?: string
  required?: boolean
  hint?: string
  minLength?: number
  autoComplete?: string
}) {
  return (
    <label className="form-field">
      <span className="field-label">{label}</span>
      <input
        className="field"
        value={value}
        type={type}
        required={required}
        minLength={minLength}
        autoComplete={autoComplete}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
      />
      {hint && <small className="field-hint">{hint}</small>}
    </label>
  )
}

export function DialogActions({
  onClose,
  label,
  disabled = false,
  leading,
  danger = false,
}: {
  onClose: () => void
  label: string
  disabled?: boolean
  leading?: ReactNode
  danger?: boolean
}) {
  const t = useTranslation()
  return (
    <div className="dialog-actions">
      {leading}
      <button type="button" className="button secondary" onClick={onClose}>
        {t('cancel')}
      </button>
      <button
        type="submit"
        className={`button ${danger ? 'danger-solid' : 'primary'}`}
        disabled={disabled}
      >
        {label}
      </button>
    </div>
  )
}

export function Toggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean
  onChange: () => void
  label: string
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={onChange}
      className={`toggle ${checked ? 'enabled' : ''}`}
    >
      <span />
    </button>
  )
}
