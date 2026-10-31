export const TIMELINE_INCLUDE_PERSONAL_KEY = 'picasu-timeline-include-personal'

export function getIncludePersonal(): boolean {
  try {
    return localStorage.getItem(TIMELINE_INCLUDE_PERSONAL_KEY) === 'true'
  } catch {
    return false
  }
}

export function setIncludePersonal(value: boolean): void {
  try {
    if (value) {
      localStorage.setItem(TIMELINE_INCLUDE_PERSONAL_KEY, 'true')
    } else {
      localStorage.removeItem(TIMELINE_INCLUDE_PERSONAL_KEY)
    }
  } catch {
    // Best-effort persistence; in-memory toggle still drives this session.
  }
}
