// ergo's service worker: shows web push notifications sent by the Web push step.
// It only handles pushes and clicks; it doesn't cache or intercept requests.

self.addEventListener('install', () => self.skipWaiting())
self.addEventListener('activate', (event) => event.waitUntil(self.clients.claim()))

self.addEventListener('push', (event) => {
  let data = {}
  try {
    data = event.data ? event.data.json() : {}
  } catch {
    data = { body: event.data ? event.data.text() : '' }
  }
  const title = data.title || 'Ergo'
  event.waitUntil(
    self.registration.showNotification(title, {
      body: data.body || '',
      icon: 'icon.svg',
      badge: 'icon.svg',
      // One notification per workflow: a newer one replaces the older.
      tag: data.tag || undefined,
      renotify: !!data.tag,
      // High urgency stays on screen until it's dismissed.
      requireInteraction: data.urgency === 'high',
      data: { url: data.url || '' },
    }),
  )
})

self.addEventListener('notificationclick', (event) => {
  event.notification.close()
  // A link from the step, or ergo itself (the folder this worker lives in).
  const target = new URL(event.notification.data?.url || './', self.registration.scope).href
  event.waitUntil(
    (async () => {
      const windows = await self.clients.matchAll({ type: 'window', includeUncontrolled: true })
      const open = windows.find((w) => w.url.startsWith(target))
      if (open) return open.focus()
      return self.clients.openWindow(target)
    })(),
  )
})
