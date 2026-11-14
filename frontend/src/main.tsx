import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { clearLegacyOnboarding } from './onboarding'
import { WorkspaceProvider } from './workspace'
import { GatewayGate } from './components/GatewayGate'
import './styles.css'

clearLegacyOnboarding()

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <WorkspaceProvider><GatewayGate><App /></GatewayGate></WorkspaceProvider>
  </React.StrictMode>,
)
