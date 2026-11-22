import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import { Check, X } from 'lucide-react'
import { translations, type Language } from '../i18n'
import { translate } from '../translation'
import { readSettings } from '../gateway-workspace'

type ToastContextValue = {
  notify: (message: string) => void
  clearToast: () => void
}

const ToastContext = createContext<ToastContextValue | null>(null)

// Rendered above WorkspaceProvider, so the toast reads the persisted browser
// language instead of useTranslation/useWorkspace (which would throw here).
function Toast({ message, onClose }: { message: string; onClose: () => void }) {
  const dismiss = translate(translations, readSettings().language as Language, 'dismiss')
  return <div className="toast" role="status"><span className="toast-check"><Check size={14} /></span>{message}<button onClick={onClose} aria-label={dismiss}><X size={14} /></button></div>
}

export function WorkspaceToastProvider({ children }: { children: React.ReactNode }) {
  const [message, setMessage] = useState('')
  const notify = useCallback((next: string) => setMessage(next), [])
  const clearToast = useCallback(() => setMessage(''), [])
  const actions = useMemo(() => ({ notify, clearToast }), [notify, clearToast])

  useEffect(() => {
    if (!message) return
    const timer = setTimeout(() => setMessage(''), 3500)
    return () => clearTimeout(timer)
  }, [message])

  return (
    <ToastContext.Provider value={actions}>
      {children}
      {message && <Toast message={message} onClose={clearToast} />}
    </ToastContext.Provider>
  )
}

export function useWorkspaceToast() {
  const context = useContext(ToastContext)
  if (!context) throw new Error('useWorkspaceToast must be used within WorkspaceToastProvider')
  return context
}
