import type { Language } from './i18n'

export const ONBOARDING_KEY = 'sepia.onboarding.v1'

export type OnboardingDraft = {
  workspace: string
  language: Language
  deployment: 'local' | 'cloud'
  protocol: 'http' | 'https'
  host: string
  port: string
}

export function isOnboardingComplete(value: string | null) {
  return value === 'complete'
}

export function completeOnboarding<T extends { workspace: string }>(settings: T, draft: OnboardingDraft): T {
  return {
    ...settings,
    workspace: draft.workspace.trim(),
    language: draft.language,
    deployment: draft.deployment,
    protocol: draft.deployment === 'local' ? 'http' : draft.protocol,
    host: draft.host.trim(),
    port: draft.port,
  }
}
