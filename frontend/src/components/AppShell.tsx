import { useEffect, useMemo, useState } from 'react'
import {
  ArrowUpRight,
  BarChart3,
  Blocks,
  ChevronRight,
  CircleHelp,
  Command,
  ExternalLink,
  GitBranch,
  Inbox,
  Layers3,
  LayoutDashboard,
  Menu as MenuIcon,
  Moon,
  Search,
  Settings,
  Store,
  Sun,
  X,
} from 'lucide-react'
import { Dialog } from '@headlessui/react'
import { useTranslation, useWorkspace } from '../workspace'
import { StatusDot } from './UI'

export type Page =
  | 'Overview'
  | 'Providers'
  | 'Plaza'
  | 'Routing'
  | 'Plugins'
  | 'Requests'
  | 'Analytics'
  | 'Settings'

export const navItems = [
  { label: 'Overview', icon: LayoutDashboard },
  { label: 'Providers', icon: Layers3 },
  { label: 'Plaza', icon: Store },
  { label: 'Routing', icon: GitBranch },
  { label: 'Plugins', icon: Blocks },
] as const

export const insightItems = [
  { label: 'Requests', icon: Inbox },
  { label: 'Analytics', icon: BarChart3 },
] as const

export function Logo({ compact = false }: { compact?: boolean }) {
  return (
    <div className="brand">
      <span className="brand-symbol" aria-hidden="true">
        <i />
        <i />
        <i />
      </span>
      {!compact && (
        <span>
          sepia<span className="brand-dot">.</span>
        </span>
      )}
    </div>
  )
}

export function Sidebar({
  page,
  mobileOpen,
  onCloseMobile,
  onNavigate,
  onOpenHelp,
}: {
  page: Page
  mobileOpen: boolean
  onCloseMobile: () => void
  onNavigate: (page: Page) => void
  onOpenHelp: () => void
}) {
  const { settings, providers, plugins } = useWorkspace()
  const t = useTranslation()
  const enabledCount = providers.filter((provider) => provider.enabled).length

  return (
    <>
      {mobileOpen && (
        <button
          type="button"
          className="mobile-overlay"
          onClick={onCloseMobile}
          aria-label={t('closeNavigation')}
        />
      )}
      <aside className={`sidebar ${mobileOpen ? 'mobile-open' : ''}`}>
        <div className="sidebar-top">
          <Logo />
          <button
            type="button"
            className="mobile-close icon-button"
            onClick={onCloseMobile}
            aria-label={t('closeNavigation')}
          >
            <X size={18} />
          </button>
        </div>

        <div className="sidebar-workspace">
          <span className="sidebar-workspace-mark" aria-hidden="true">
            {settings.workspace.charAt(0).toUpperCase()}
          </span>
          <div className="sidebar-workspace-copy">
            <strong>{settings.workspace}</strong>
            <span>{t('browserWorkspace')}</span>
          </div>
        </div>

        <nav className="sidebar-nav" aria-label={t('workspace')}>
          <span className="nav-label">{t('workspace')}</span>
          {navItems.map(({ label, icon: Icon }) => (
            <button
              type="button"
              key={label}
              className={`nav-item ${page === label ? 'active' : ''}`}
              aria-current={page === label ? 'page' : undefined}
              onClick={() => onNavigate(label)}
            >
              <Icon size={17} />
              <span>{t(label.toLowerCase() as 'overview')}</span>
              {label === 'Providers' && providers.length > 0 && (
                <span className="nav-count tabular">{providers.length}</span>
              )}
              {label === 'Plugins' && plugins.length > 0 && (
                <span className="nav-count tabular">{plugins.length}</span>
              )}
            </button>
          ))}

          <span className="nav-label insights-label">{t('insights')}</span>
          {insightItems.map(({ label, icon: Icon }) => (
            <button
              type="button"
              key={label}
              className={`nav-item ${page === label ? 'active' : ''}`}
              aria-current={page === label ? 'page' : undefined}
              onClick={() => onNavigate(label)}
            >
              <Icon size={17} />
              <span>{t(label.toLowerCase() as 'overview')}</span>
            </button>
          ))}
        </nav>

        <div className="sidebar-bottom">
          <button type="button" className="nav-item" onClick={onOpenHelp}>
            <CircleHelp size={17} />
            <span>{t('helpDocs')}</span>
            <ExternalLink size={12} className="nav-trailing-icon" />
          </button>
          <button
            type="button"
            className={`nav-item ${page === 'Settings' ? 'active' : ''}`}
            aria-current={page === 'Settings' ? 'page' : undefined}
            onClick={() => onNavigate('Settings')}
          >
            <Settings size={17} />
            <span>{t('settings')}</span>
          </button>

          <div className="gateway-card">
            <span className="gateway-live-row">
              <StatusDot />
              <span>{t('gatewayOperational')}</span>
            </span>
            <code className="mono">{settings.host}:{settings.port}</code>
            <small className="tabular">
              {enabledCount} / {providers.length} {t('providersEnabledShort')}
            </small>
          </div>
        </div>
      </aside>
    </>
  )
}

export function Topbar({
  page,
  onOpenMobile,
  onOpenSearch,
  onOpenWorkspace,
}: {
  page: Page
  onOpenMobile: () => void
  onOpenSearch: () => void
  onOpenWorkspace: () => void
}) {
  const { settings, setSettings, session } = useWorkspace()
  const t = useTranslation()
  const isLight =
    settings.theme === 'Light' ||
    (settings.theme === 'System' &&
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-color-scheme: light)').matches)

  const toggleTheme = () => {
    setSettings({
      ...settings,
      theme: isLight ? 'Sepia dark' : 'Light',
    })
  }

  return (
    <header className="topbar">
      <div className="topbar-left">
        <button
          type="button"
          className="mobile-menu icon-button"
          onClick={onOpenMobile}
          aria-label={t('openNavigation')}
        >
          <MenuIcon size={20} />
        </button>
        <nav className="breadcrumb" aria-label="Breadcrumb">
          <span>{t('workspace')}</span>
          <ChevronRight size={13} aria-hidden="true" />
          <strong>{t(page.toLowerCase() as 'overview')}</strong>
        </nav>
      </div>

      <div className="topbar-actions">
        <button type="button" className="top-search" onClick={onOpenSearch}>
          <Search size={15} />
          <span>{t('searchAnything')}</span>
          <kbd>
            <Command size={11} /> K
          </kbd>
        </button>
        <button
          type="button"
          className="icon-button top-theme-toggle"
          onClick={toggleTheme}
          aria-label={isLight ? t('darkTheme') : t('lightTheme')}
          title={isLight ? t('darkTheme') : t('lightTheme')}
        >
          {isLight ? <Moon size={16} /> : <Sun size={16} />}
        </button>
        <button
          type="button"
          className="top-avatar"
          onClick={onOpenWorkspace}
          aria-label={t('workspaceProfile')}
        >
          {(session?.username || settings.workspace).charAt(0).toUpperCase()}
        </button>
      </div>
    </header>
  )
}

export function CommandPalette({
  open,
  onClose,
  onNavigate,
}: {
  open: boolean
  onClose: () => void
  onNavigate: (page: Page) => void
}) {
  const { providers, plugins, routes, requests } = useWorkspace()
  const t = useTranslation()
  const [query, setQuery] = useState('')
  const [activeIndex, setActiveIndex] = useState(0)

  const pageItems: { label: string; page: Page; type: string; icon: typeof Settings }[] = useMemo(
    () => [
      ...navItems.map((item) => ({
        label: t(item.label.toLowerCase() as 'overview'),
        page: item.label as Page,
        type: t('page'),
        icon: item.icon,
      })),
      ...insightItems.map((item) => ({
        label: t(item.label.toLowerCase() as 'overview'),
        page: item.label as Page,
        type: t('page'),
        icon: item.icon,
      })),
      { label: t('settings'), page: 'Settings' as Page, type: t('page'), icon: Settings },
    ],
    [t],
  )

  const entityItems = useMemo(
    () => [
      ...providers.map((item) => ({
        label: item.name,
        type: t('provider'),
        page: 'Providers' as Page,
        icon: Layers3,
      })),
      ...routes.map((item) => ({
        label: item.name || item.slug,
        type: t('route'),
        page: 'Routing' as Page,
        icon: GitBranch,
      })),
      ...plugins.map((item) => ({
        label: item.name,
        type: t('plugins'),
        page: 'Plugins' as Page,
        icon: Blocks,
      })),
      ...requests.slice(0, 20).map((item) => ({
        label: item.model,
        type: t('requests'),
        page: 'Requests' as Page,
        icon: Inbox,
      })),
    ],
    [providers, routes, plugins, requests, t],
  )

  const results = useMemo(() => {
    const needle = query.trim().toLowerCase()
    const combined = [...pageItems, ...entityItems]
    if (!needle) return combined.slice(0, 10)
    return combined
      .filter((item) => `${item.label} ${item.type}`.toLowerCase().includes(needle))
      .slice(0, 12)
  }, [pageItems, entityItems, query])

  useEffect(() => {
    setActiveIndex(0)
  }, [query, open])

  const selectResult = (target: Page) => {
    onNavigate(target)
    onClose()
    setQuery('')
  }

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (!results.length) return
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      setActiveIndex((current) => (current + 1) % results.length)
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      setActiveIndex((current) => (current - 1 + results.length) % results.length)
    } else if (event.key === 'Enter') {
      event.preventDefault()
      const chosen = results[activeIndex] || results[0]
      if (chosen) selectResult(chosen.page)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} className="dialog-root">
      <div className="dialog-backdrop" aria-hidden="true" />
      <div className="dialog-position search-position">
        <Dialog.Panel className="dialog-panel search-dialog glass-panel">
          <div className="search-dialog-input">
            <Search size={18} />
            <input
              autoFocus
              placeholder={t('searchPlaceholder')}
              aria-label={t('searchWorkspace')}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={handleKeyDown}
            />
            <button
              type="button"
              className="icon-button"
              onClick={onClose}
              aria-label={t('close')}
            >
              <X size={16} />
            </button>
          </div>
          <div className="search-results" role="listbox">
            <span className="nav-label">{t('page')}</span>
            {results.map(({ label, icon: Icon, type, page: target }, index) => (
              <button
                type="button"
                role="option"
                aria-selected={index === activeIndex}
                className={index === activeIndex ? 'is-focused' : ''}
                key={`${type}-${label}-${index}`}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => selectResult(target)}
              >
                <Icon size={16} />
                <span>{label}</span>
                <small>{type}</small>
                <ArrowUpRight size={14} />
              </button>
            ))}
            {!results.length && <p>{t('searchEmpty')}</p>}
          </div>
          <div className="search-footer">
            <span>
              <kbd>↵</kbd> {t('toSelect')}
            </span>
            <span>
              <kbd>esc</kbd> {t('toClose')}
            </span>
          </div>
        </Dialog.Panel>
      </div>
    </Dialog>
  )
}
