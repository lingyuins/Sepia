import { useState } from 'react'
import { Check, Pencil, Plus, Trash2 } from 'lucide-react'
import { api } from '../api'
import { scopedModels } from '../key-models'
import { useTranslation, useWorkspace } from '../workspace'
import { DialogActions, Field, Modal } from './UI'

type EditorState = { id: number | null; name: string; description: string; members: string[] }

export default function ModelGroupsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { providers, routes, modelGroups, run, notify } = useWorkspace()
  const t = useTranslation()
  const options = scopedModels(providers, routes)
  const [editor, setEditor] = useState<EditorState | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  const startCreate = () => { setError(''); setEditor({ id: null, name: '', description: '', members: [] }) }
  const startEdit = (group: { id: number; name: string; description: string; members: string[] }) => {
    setError('')
    setEditor({ id: group.id, name: group.name, description: group.description, members: [...group.members] })
  }
  const toggleMember = (value: string) => {
    setEditor((current) => current && ({
      ...current,
      members: current.members.includes(value) ? current.members.filter((item) => item !== value) : [...current.members, value],
    }))
  }
  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!editor || busy) return
    if (!editor.name.trim()) { setError(t('modelGroupName')); return }
    setBusy(true)
    try {
      await run(() => api(editor.id === null ? '/api/v1/model-groups' : `/api/v1/model-groups/${editor.id}`, {
        method: editor.id === null ? 'POST' : 'PUT',
        body: JSON.stringify({ name: editor.name.trim(), description: editor.description.trim(), members: editor.members }),
      }), ['modelGroups'])
      notify(t('modelGroupSaved'))
      setEditor(null)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }
  const remove = async (id: number) => {
    try {
      await run(() => api(`/api/v1/model-groups/${id}`, { method: 'DELETE' }), ['modelGroups'])
      notify(t('modelGroupDeleted'))
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    }
  }

  if (editor) {
    return (
      <Modal open={open} onClose={() => setEditor(null)} title={editor.id === null ? t('modelGroupAdd') : t('modelGroupEdit')} description={t('modelGroupsHint')} size="key-policy-dialog">
        <form className="model-group-editor" onSubmit={save}>
          <div className="key-policy-grid">
            <Field label={t('modelGroupName')} value={editor.name} onChange={(value) => setEditor((current) => current && ({ ...current, name: value }))} required />
            <Field label={t('modelGroupDescription')} value={editor.description} onChange={(value) => setEditor((current) => current && ({ ...current, description: value }))} />
          </div>
          <fieldset className="key-policy-field">
            <legend>{t('modelGroupMembers')}</legend>
            <div className="key-selection-actions">
              <button type="button" onClick={() => setEditor((current) => current && ({ ...current, members: options.map((option) => option.value) }))}>{t('selectAll')}</button>
              <button type="button" onClick={() => setEditor((current) => current && ({ ...current, members: [] }))}>{t('clear')}</button>
              <span>{editor.members.length} / {options.length}</span>
            </div>
            <div className="key-option-grid">
              {options.map((option) => (
                <label className={editor.members.includes(option.value) ? 'selected' : ''} key={option.value}>
                  <input type="checkbox" checked={editor.members.includes(option.value)} onChange={() => toggleMember(option.value)} />
                  <span>{option.label}{option.route && <small>{option.detail}</small>}</span>
                  {editor.members.includes(option.value) && <Check size={13} />}
                </label>
              ))}
            </div>
            <small>{t('modelGroupMembersHint')}</small>
          </fieldset>
          {error && <p className="key-policy-error" role="alert">{error}</p>}
          <DialogActions onClose={() => setEditor(null)} label={t('saveChanges')} disabled={busy} />
        </form>
      </Modal>
    )
  }

  return (
    <Modal open={open} onClose={onClose} title={t('modelGroups')} description={t('modelGroupsHint')} size="key-policy-dialog">
      <div className="model-group-editor">
        <div className="key-selection-actions">
          <button type="button" className="button secondary" onClick={startCreate}><Plus size={14} />{t('modelGroupAdd')}</button>
        </div>
        {modelGroups.length ? (
          <div className="model-group-list">
            {modelGroups.map((group) => (
              <div className="model-group-row" key={group.id}>
                <div>
                  <strong>{group.name}</strong>
                  {group.description && <p>{group.description}</p>}
                  <div className="model-chip-list">
                    {group.members.map((member) => (
                      <span className="model-chip" key={member}><code className="mono">{member}</code></span>
                    ))}
                  </div>
                </div>
                <button type="button" className="icon-button" onClick={() => startEdit(group)} aria-label={t('modelGroupEdit')}><Pencil size={15} /></button>
                <button type="button" className="icon-button danger-icon" onClick={() => remove(group.id)} aria-label={t('deleteNamed', { name: group.name })}><Trash2 size={15} /></button>
              </div>
            ))}
          </div>
        ) : <p className="key-empty-options">{t('modelGroupsEmpty')}</p>}
        <DialogActions onClose={onClose} label={t('close')} />
      </div>
    </Modal>
  )
}