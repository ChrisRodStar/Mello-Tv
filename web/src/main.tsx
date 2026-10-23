import './lib/browser-compat';

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App';
import { apiPlaybackResolver } from './lib/playback';

import './styles.css';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App resolvePlayback={apiPlaybackResolver} />
  </StrictMode>,
);
