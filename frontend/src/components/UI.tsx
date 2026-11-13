import { Dialog } from '@headlessui/react'
import { X } from 'lucide-react'
import { useTranslation } from '../workspace'

export function Modal({ open, onClose, title, description, children, size = '' }: { open: boolean; onClose: () => void; title: string; description?: string; children: React.ReactNode; size?: string }) {
  return <Dialog open={open} onClose={onClose} className="dialog-root"><div className="dialog-backdrop" aria-hidden="true" /><div className="dialog-position"><Dialog.Panel className={`dialog-panel glass-panel ${size}`}><button type="button" className="icon-button dialog-close" onClick={onClose} aria-label={useTranslation()('close')}><X size={18} /></button><Dialog.Title>{title}</Dialog.Title>{description && <Dialog.Description>{description}</Dialog.Description>}{children}</Dialog.Panel></div></Dialog>
}
export function Field({ label, value, onChange, placeholder = '', type = 'text', required = false, hint }: { label: string; value: string; onChange: (value: string) => void; placeholder?: string; type?: string; required?: boolean; hint?: string }) {
  return <label className="form-field"><span className="field-label">{label}</span><input className="field" value={value} type={type} required={required} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} />{hint && <small className="field-hint">{hint}</small>}</label>
}
export function DialogActions({ onClose, label, disabled = false }: { onClose: () => void; label: string; disabled?: boolean }) {
  return <div className="dialog-actions"><button type="button" className="button secondary" onClick={onClose}>{useTranslation()('cancel')}</button><button type="submit" className="button primary" disabled={disabled}>{label}</button></div>
}
export function Toggle({ checked, onChange, label }: { checked: boolean; onChange: () => void; label: string }) {
  return <button type="button" role="switch" aria-checked={checked} aria-label={label} onClick={onChange} className={`toggle ${checked ? 'enabled' : ''}`}><span /></button>
}
