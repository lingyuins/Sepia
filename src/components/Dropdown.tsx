import { useEffect, useRef, useState } from 'react'
import { Check, ChevronDown, Layers3, Plus, X } from 'lucide-react'
import { useTranslation } from '../workspace'

export type DropdownOption = { value: string; label: string; description?: string; badge?: string }
type DropdownProps = {
  label: string
  value: string
  options: DropdownOption[]
  onChange: (value: string) => void
  onCreate?: () => void
  placeholder?: string
  variant?: 'field' | 'compact'
  icon?: React.ReactNode
  className?: string
}
export function Dropdown({ label, value, options, onChange, onCreate, placeholder, variant = 'field', icon, className = '' }: DropdownProps) {
  const t = useTranslation()
  const [open, setOpen] = useState(false)
  const [closing, setClosing] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  const closeTimer = useRef<number | undefined>(undefined)
  const selected = options.find((option) => option.value === value)
  const closeMenu = () => {
    if (!open || closeTimer.current !== undefined) return
    setClosing(true)
    closeTimer.current = window.setTimeout(() => { setOpen(false); setClosing(false); closeTimer.current = undefined }, 150)
  }
  useEffect(() => {
    const close = (event: MouseEvent) => { if (root.current && !root.current.contains(event.target as Node)) closeMenu() }
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') closeMenu() }
    document.addEventListener('mousedown', close)
    document.addEventListener('keydown', escape)
    return () => { document.removeEventListener('mousedown', close); document.removeEventListener('keydown', escape); window.clearTimeout(closeTimer.current) }
  }, [open])
  return <div className={`form-field dropdown-field ${variant === 'compact' ? 'compact' : ''} ${className}`} ref={root}>
    <span className={`field-label ${variant === 'compact' ? 'sr-only' : ''}`} id={`dropdown-${label}`}>{label}</span>
    <div className="dropdown-anchor">
      <button type="button" className={`dropdown-trigger ${open ? 'is-open' : ''}`} aria-labelledby={`dropdown-${label}`} aria-haspopup="listbox" aria-expanded={open} onClick={() => { if (open) closeMenu(); else { setOpen(true); setClosing(false) } }}>
        <span className="dropdown-selected">{(icon || variant === 'field') && <span className="dropdown-selected-icon">{icon || <Layers3 size={16} />}</span>}<span><strong>{selected?.label || placeholder || value}</strong>{selected?.description && <small>{selected.description}</small>}</span></span><ChevronDown size={15} className={open && !closing ? 'rotated' : ''} />
      </button>
      {open && <div className={`dropdown-menu ${closing ? 'is-closing' : ''}`} role="listbox" aria-label={label}>
        {variant === 'field' && <div className="dropdown-menu-top"><span>{label}</span><button type="button" className="icon-button" onClick={closeMenu} aria-label={t('close')}><X size={13} /></button></div>}
        <div className="dropdown-options">{options.map((option) => <button type="button" role="option" aria-selected={value === option.value} className={`dropdown-option ${value === option.value ? 'selected' : ''}`} key={option.value} onClick={() => { onChange(option.value); closeMenu() }}>{variant === 'field' && <span className="dropdown-option-icon"><Layers3 size={15} /></span>}<span className="dropdown-option-copy"><strong title={option.label}>{option.label}</strong>{option.description && <small>{option.description}</small>}</span>{option.badge && <span className="dropdown-badge">{option.badge}</span>}<span className="dropdown-check">{value === option.value && <Check size={15} />}</span></button>)}</div>
        {onCreate && <button type="button" className="dropdown-create" onClick={() => { closeMenu(); onCreate() }}><Plus size={15} />{t('createGroup')}</button>}
      </div>}
    </div>
  </div>
}
