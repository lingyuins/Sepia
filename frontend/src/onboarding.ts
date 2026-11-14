export const ONBOARDING_KEY = 'sepia.onboarding.v1'

export function clearLegacyOnboarding() {
  try {
    localStorage.removeItem(ONBOARDING_KEY)
  } catch {
    // Storage may be disabled; gateway authentication must still start.
  }
}
