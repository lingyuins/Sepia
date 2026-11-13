import React from 'react'
import ReactDOM from 'react-dom/client'
import { useState } from 'react'
import App from './App'
import Onboarding from './components/Onboarding'
import { isOnboardingComplete, ONBOARDING_KEY } from './onboarding'
import { WorkspaceProvider } from './workspace'
import { GatewayGate } from './components/GatewayGate'
import './styles.css'

function Root() {
  const [onboarded, setOnboarded] = useState(() => { try { return isOnboardingComplete(localStorage.getItem(ONBOARDING_KEY)) } catch { return false } })
  const complete = () => { try { localStorage.setItem(ONBOARDING_KEY, 'complete') } catch { /* Storage may be disabled by the browser. */ } setOnboarded(true) }
  return onboarded ? <GatewayGate><App /></GatewayGate> : <Onboarding onComplete={complete} />
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <WorkspaceProvider><Root /></WorkspaceProvider>
  </React.StrictMode>,
)
