/// Secure-context clipboard copy. The async Clipboard API is only
/// available in secure contexts (https, localhost); elsewhere
/// navigator.clipboard is undefined. There is deliberately no execCommand
/// fallback — it is deprecated and must not be the default copy path.
/// Callers gate the Copy button on canShowCopyButton and guide manual
/// copy otherwise. Dependencies are injectable so the gating is testable
/// without DOM globals.
export interface ClipboardDeps {
  secureContext: boolean
  clipboard?: { writeText(text: string): Promise<void> } | undefined
}

const globalDeps = (): ClipboardDeps => {
  const g = globalThis as unknown as {
    isSecureContext?: boolean
    navigator?: { clipboard?: { writeText(text: string): Promise<void> } }
  }
  return {
    secureContext: g.isSecureContext === true,
    clipboard: g.navigator?.clipboard
  }
}

/// Whether the Copy button may render: secure context plus Clipboard API.
export const canShowCopyButton = (deps: ClipboardDeps = globalDeps()): boolean =>
  deps.secureContext && deps.clipboard !== undefined

/// Copy text via the secure Clipboard API. Resolves false without writing
/// when the context is insecure, the API is missing, or the write rejects.
export const copyTextToClipboard = async (
  text: string,
  deps: ClipboardDeps = globalDeps()
): Promise<boolean> => {
  const clipboard = deps.clipboard
  if (!deps.secureContext || clipboard === undefined) return false
  try {
    await clipboard.writeText(text)
    return true
  } catch {
    return false
  }
}
