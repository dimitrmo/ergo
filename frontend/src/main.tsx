import '@fontsource-variable/inter'
import '@fontsource/ibm-plex-mono/400.css'
import '@xyflow/react/dist/base.css'
import './styles.css'

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
