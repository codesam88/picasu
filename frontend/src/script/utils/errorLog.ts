// Tiny in-memory ring buffer of recent client-side error messages.
// Attached to bug reports. Stores message text only (no URLs, stacks, etc.).

const MAX_ENTRIES = 20
let buffer: string[] = []

function hasMessage(value: unknown): value is { message: unknown } {
  return (
    typeof value === 'object' &&
    value !== null &&
    Object.prototype.hasOwnProperty.call(value, 'message')
  )
}

function pushMessage(message: unknown): void {
  try {
    let text: string
    if (typeof message === 'string') {
      text = message
    } else if (hasMessage(message) && typeof message.message === 'string') {
      text = message.message
    } else {
      text = String(message)
    }
    buffer.push(text)
    if (buffer.length > MAX_ENTRIES) {
      buffer = buffer.slice(buffer.length - MAX_ENTRIES)
    }
  } catch {
    // Never let logging itself throw.
  }
}

export function clearErrorLog(): void {
  buffer = []
}

export function getRecentErrors(): string[] {
  return [...buffer]
}

export function installErrorLog(): void {
  try {
    if (typeof window === 'undefined') return
    window.onerror = (message) => {
      try {
        pushMessage(message)
      } catch {
        // ignore
      }
      return false
    }
    window.addEventListener('unhandledrejection', (event: PromiseRejectionEvent) => {
      try {
        const reason: unknown = (event as { reason?: unknown }).reason
        if (hasMessage(reason) && typeof reason.message === 'string') {
          pushMessage(reason.message)
        } else {
          pushMessage(reason)
        }
      } catch {
        // ignore
      }
    })
  } catch {
    // Never throw from installation.
  }
}
