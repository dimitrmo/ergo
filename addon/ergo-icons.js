// Registers the "ergo" icon set with the Home Assistant frontend, so the
// add-on's sidebar entry (panel_icon: ergo:logo) shows the ergo mark.
//
// HA icons are one colour and fill only, so this is the mark without the
// tile, gradient or animation: trigger -> step -> output on one path.
// Generated from the geometry in frontend/src/components/ErgoLogo.tsx.
//
// Load it on every page, not as a dashboard resource:
//
//   frontend:
//     extra_module_url:
//       - /local/ergo-icons.js

const ICONS = {
  logo: 'M14.25 5.48L14.23 6.11L15.23 6.93L16.16 7.89L16.99 8.94L17.7 10.06L18.17 10.96L18.57 11.9L18.9 12.84L19.13 13.76L19.74 13.95L20.07 14.1L20.66 14.5L21.17 15.01L21.57 15.6L21.84 16.26L21.93 16.61L21.98 16.97L21.98 17.68L21.84 18.39L21.72 18.72L21.38 19.35L21.17 19.64L20.93 19.91L20.38 20.36L19.74 20.7L19.41 20.82L18.7 20.96L17.99 20.96L17.29 20.82L16.62 20.55L16.32 20.36L15.76 19.91L15.31 19.35L14.97 18.72L14.85 18.39L14.71 17.68L14.71 16.97L14.85 16.26L15.12 15.6L15.31 15.3L15.76 14.74L16.32 14.29L16.62 14.1L17.01 13.93L16.75 13.06L16.45 12.27L15.98 11.29L15.52 10.53L15.01 9.8L14.43 9.11L13.79 8.47L13.16 7.93L12.82 8.15L12.33 8.35L11.8 8.46L11.54 8.47L11.27 8.46L10.74 8.35L10.25 8.15L10 8L9.36 8.6L8.68 9.38L8.22 10.02L7.66 10.93L7.16 11.92L6.74 12.96L6.38 14.06L6.11 15.17L6.45 15.4L6.65 15.58L6.99 16L7.24 16.47L7.4 16.98L7.44 17.24L7.44 17.78L7.33 18.3L7.13 18.8L6.83 19.24L6.45 19.62L6.01 19.91L5.52 20.12L4.99 20.22L4.46 20.22L3.93 20.12L3.44 19.91L3 19.62L2.8 19.44L2.46 19.02L2.21 18.55L2.05 18.04L2.01 17.78L2.01 17.24L2.05 16.98L2.21 16.47L2.46 16L2.8 15.58L3 15.4L3.44 15.11L4.02 14.88L4.27 13.8L4.65 12.56L4.99 11.63L5.51 10.48L5.82 9.9L6.3 9.11L7.02 8.09L7.85 7.13L8.29 6.7L8.85 6.21L8.81 5.75L8.82 5.48L8.93 4.96L9.13 4.46L9.43 4.02L9.81 3.64L10.02 3.48L10.25 3.34L10.74 3.14L11.27 3.03L11.54 3.02L12.07 3.07L12.33 3.14L12.82 3.34L13.05 3.48L13.46 3.82L13.8 4.23L13.94 4.46L14.14 4.96ZM20.01 17.16L19.95 16.84L19.82 16.54L19.64 16.26L19.41 16.03L19.13 15.85L18.83 15.73L18.51 15.66L18.18 15.66L17.86 15.73L17.56 15.85L17.29 16.03L17.05 16.26L16.87 16.54L16.75 16.84L16.68 17.16L16.68 17.49L16.75 17.81L16.87 18.11L17.05 18.39L17.29 18.62L17.56 18.8L17.86 18.92L18.18 18.99L18.51 18.99L18.83 18.92L19.13 18.8L19.41 18.62L19.64 18.39L19.82 18.11L19.95 17.81L20.01 17.49Z',
}

window.customIcons = window.customIcons || {}
window.customIcons.ergo = {
  getIcon: async (name) => ({ path: ICONS[name] }),
  getIconList: async () => Object.keys(ICONS).map((name) => ({ name })),
}

// An <ha-icon> looks its icon set up once. One drawn before this module
// loaded (the sidebar often is) gave up and stays blank, so redraw those.
// Icons drawn from here on find the set themselves.
function* deep(root) {
  for (const el of root.querySelectorAll('*')) {
    yield el
    if (el.shadowRoot) yield* deep(el.shadowRoot)
  }
}

for (const el of deep(document)) {
  if (el.localName !== 'ha-icon' || !el.icon?.startsWith('ergo:')) continue
  const icon = el.icon
  // `_legacy` is ha-icon's "unknown icon set" flag; it isn't cleared on reload.
  el._legacy = false
  el.icon = ''
  el.updateComplete.then(() => {
    el.icon = icon
  })
}
