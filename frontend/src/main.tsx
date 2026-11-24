import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { clearLegacyOnboarding } from './onboarding'
import { WorkspaceProvider } from './workspace'
import { GatewayGate } from './components/GatewayGate'
import { WorkspaceToastProvider } from './components/WorkspaceToast'
import '@fontsource-variable/manrope'
import '@fontsource-variable/dm-sans'
import '@fontsource-variable/jetbrains-mono'
import './styles.css'

clearLegacyOnboarding()

// PWA: register the shell service worker in production builds only, so the
// Vite dev server never serves cached assets.
if (import.meta.env.PROD && 'serviceWorker' in navigator) {
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('/sw.js').catch(() => undefined)
  })
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <WorkspaceToastProvider>
      <WorkspaceProvider><GatewayGate><App /></GatewayGate></WorkspaceProvider>
    </WorkspaceToastProvider>
  </React.StrictMode>,
)
