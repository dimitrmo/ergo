// Web push in this browser: the service worker, permission and subscription.

import { api } from './api.ts'

/** Why this browser can't get notifications, or null when it can. */
export function pushUnavailable(): string | null {
  if (!window.isSecureContext)
    return 'Browser notifications need Home Assistant over HTTPS, e.g. through Home Assistant Cloud or your own certificate.'
  if (!('serviceWorker' in navigator) || !('PushManager' in window) || !('Notification' in window))
    return "This browser doesn't support web push notifications."
  return null
}

async function registration(): Promise<ServiceWorkerRegistration> {
  // Relative, so it works under Home Assistant's Ingress path too.
  await navigator.serviceWorker.register('sw.js', { scope: './' })
  return navigator.serviceWorker.ready
}

/** This browser's push subscription, if it has one. */
export async function currentSubscription(): Promise<PushSubscription | null> {
  if (pushUnavailable()) return null
  const reg = await navigator.serviceWorker.getRegistration('./')
  return (await reg?.pushManager.getSubscription()) ?? null
}

/** "Chrome on Linux", as a starting name for this browser. */
export function guessBrowserName(): string {
  const ua = navigator.userAgent
  const browser = /Edg\//.test(ua)
    ? 'Edge'
    : /Firefox\//.test(ua)
      ? 'Firefox'
      : /Chrome\//.test(ua)
        ? 'Chrome'
        : /Safari\//.test(ua)
          ? 'Safari'
          : 'Browser'
  const os = /Android/.test(ua)
    ? 'Android'
    : /iPhone|iPad/.test(ua)
      ? 'iOS'
      : /Mac OS X/.test(ua)
        ? 'Mac'
        : /Windows/.test(ua)
          ? 'Windows'
          : /Linux/.test(ua)
            ? 'Linux'
            : ''
  return os ? `${browser} on ${os}` : browser
}

function keyBytes(base64url: string): Uint8Array<ArrayBuffer> {
  const b64 = (base64url + '='.repeat((4 - (base64url.length % 4)) % 4)).replace(/-/g, '+').replace(/_/g, '/')
  const raw = atob(b64)
  const bytes = new Uint8Array(new ArrayBuffer(raw.length))
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i)
  return bytes
}

/** Asks for permission, subscribes and tells ergo. Returns the browser's id. */
export async function turnOnPush(name: string, publicKey: string): Promise<string> {
  const why = pushUnavailable()
  if (why) throw new Error(why)
  const permission = await Notification.requestPermission()
  if (permission !== 'granted') throw new Error('Notifications are blocked for this page; allow them in the browser’s site settings.')
  const reg = await registration()
  // A subscription made with another key (e.g. before a reinstall) can't be reused.
  const old = await reg.pushManager.getSubscription()
  if (old) await old.unsubscribe()
  const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: keyBytes(publicKey) })
  const { id } = await api.pushSubscribe(name, sub.toJSON())
  return id
}

/** Stops notifications here and tells ergo to forget this browser. */
export async function turnOffPush(id: string): Promise<void> {
  const sub = await currentSubscription()
  await sub?.unsubscribe()
  await api.pushUnsubscribe(id)
}
