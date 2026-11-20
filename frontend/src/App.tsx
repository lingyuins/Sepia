import { useEffect, useState } from 'react'
import { useWorkspace, useTranslation, type RequestRecord } from './workspace'
import { api, providerInput } from './api'
import { ProviderDialog, Toast } from './components/Provider'
import { Modal } from './components/UI'
import {
  CommandPalette,
  Logo,
  Sidebar,
  Topbar,
  insightItems,
  navItems,
  type Page,
} from './components/AppShell'
import { formatVersion } from './self-update'
import { OverviewView } from './views/OverviewView'
import { ProvidersView } from './views/ProvidersView'
import { PlazaView } from './views/PlazaView'
import { RoutingView } from './views/RoutingView'
import { PluginsView } from './views/PluginsView'
import { RequestsView } from './views/RequestsView'
import { AnalyticsView } from './views/AnalyticsView'
import SettingsView from './components/SettingsView'
import { HelpDialog, WorkspaceDialog } from './components/ShellDialogs'
import { RequestDetails } from './components/RequestDetails'

function resolveHashPage(): Page {
  const hash = window.location.hash.slice(1)
  if (
    hash === 'Settings' ||
    [...navItems, ...insightItems].some((item) => item.label === hash)
  ) {
    return hash as Page
  }
  return 'Overview'
}

function App() {
  const { settings, providers, notify, toast, clearToast, run, version } = useWorkspace()
  const t = useTranslation()

  const [page, setPage] = useState<Page>(resolveHashPage)
  const [mobileNav, setMobileNav] = useState(false)
  const [addProvider, setAddProvider] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [helpOpen, setHelpOpen] = useState(false)
  const [workspaceOpen, setWorkspaceOpen] = useState(false)
  const [selectedRequest, setSelectedRequest] = useState<RequestRecord | null>(null)

  const navigate = (next: Page) => {
    setPage(next)
    window.location.hash = next
    setMobileNav(false)
    window.scrollTo({ top: 0, behavior: settings.reducedMotion ? 'auto' : 'smooth' })
  }

  useEffect(() => {
    const onHashChange = () => setPage(resolveHashPage())
    window.addEventListener('hashchange', onHashChange)
    return () => window.removeEventListener('hashchange', onHashChange)
  }, [])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        setSearchOpen(true)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])

  return (
    <div className="app-shell">
      <Sidebar
        page={page}
        mobileOpen={mobileNav}
        onCloseMobile={() => setMobileNav(false)}
        onNavigate={navigate}
        onOpenHelp={() => setHelpOpen(true)}
      />

      <main className="main-content">
        <Topbar
          page={page}
          onOpenMobile={() => setMobileNav(true)}
          onOpenSearch={() => setSearchOpen(true)}
          onOpenWorkspace={() => setWorkspaceOpen(true)}
        />

        <div className="page-content">
          {page === 'Overview' && (
            <OverviewView
              providers={providers}
              navigate={navigate}
              onAdd={() => setAddProvider(true)}
              onNotify={notify}
              onRequest={setSelectedRequest}
            />
          )}
          {page === 'Providers' && <ProvidersView />}
          {page === 'Plaza' && <PlazaView />}
          {page === 'Routing' && <RoutingView />}
          {page === 'Plugins' && <PluginsView />}
          {page === 'Requests' && <RequestsView onSelect={setSelectedRequest} />}
          {page === 'Analytics' && <AnalyticsView />}
          {page === 'Settings' && <SettingsView />}

          <footer className="page-footer">
            <span>
              <Logo compact />
              {t('footer')}
            </span>
            <span className="mono">Sepia {formatVersion(version)}</span>
          </footer>
        </div>
      </main>

      <ProviderDialog
        open={addProvider}
        onClose={() => setAddProvider(false)}
        onSave={async (provider, keys) => {
          await run(() =>
            api('/api/v1/providers', {
              method: 'POST',
              body: JSON.stringify(providerInput(provider, keys)),
            }),
          )
          setAddProvider(false)
          notify(t('providerConnected', { name: provider.name }))
        }}
      />

      <CommandPalette
        open={searchOpen}
        onClose={() => setSearchOpen(false)}
        onNavigate={navigate}
      />

      <Modal
        open={!!selectedRequest}
        onClose={() => setSelectedRequest(null)}
        title={t('requestDetails')}
        description={t('requestDetailsSubtitle')}
      >
        {selectedRequest && <RequestDetails request={selectedRequest} />}
      </Modal>

      <HelpDialog open={helpOpen} onClose={() => setHelpOpen(false)} />
      <WorkspaceDialog
        open={workspaceOpen}
        onClose={() => setWorkspaceOpen(false)}
        onSettings={() => navigate('Settings')}
      />
      {toast && <Toast message={toast} onClose={clearToast} />}
    </div>
  )
}

export default App
