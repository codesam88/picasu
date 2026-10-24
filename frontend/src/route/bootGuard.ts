import type { RouteLocationRaw } from 'vue-router'

import type { ServerInfo } from '@/api/serverInfo'

export interface BootGuardDeps {
  isShareTarget: boolean
  profilePresent: boolean
  toName: string | undefined
  hasInviteToken: boolean
  serverInfo: () => Promise<ServerInfo>
}

export async function decideBootRoute(deps: BootGuardDeps): Promise<RouteLocationRaw | true> {
  // Share links are capability URLs: they authenticate out-of-band and
  // must work without login — including on installs where the ping below
  // reports a closed install. Never redirect them.
  if (deps.isShareTarget) {
    return true
  }
  if (deps.profilePresent) {
    return true
  }
  let info: ServerInfo
  try {
    info = await deps.serverInfo()
  } catch {
    // A failed ping must not abort navigation nor force the enrollment
    // screen: degrade to the plain login fall-through. The login page
    // owns surfacing connectivity problems when the user acts.
    return deps.toName === 'login' ? true : { name: 'login' }
  }
  if (!info.enrolled && info.enrollmentOpen) {
    return deps.toName === 'register' ? true : { name: 'register' }
  }
  // An invite link must be able to render the registration form even on
  // an enrolled (or closed) store: the token is validated server-side.
  if (deps.hasInviteToken && deps.toName === 'register') {
    return true
  }
  // Enrolled store or closed install: sessionless means login. This also
  // absorbs the deleted RegisterPage leave-redirect — a sessionless visit
  // to /register lands on /login instead of rendering a dead form.
  return deps.toName === 'login' ? true : { name: 'login' }
}
