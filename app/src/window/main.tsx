// The window's entry: Keel's tokens, then React renders the page into #root. The BridgeProvider is
// given no bridge, so every screen asks through the preload's bridge (read only in bridge.ts).
import './styles.css';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App.tsx';
import { BridgeProvider } from './bridge.ts';

const root = document.getElementById('root');
if (root === null) throw new Error('the page has no #root');
createRoot(root).render(
  <StrictMode>
    <BridgeProvider>
      <App />
    </BridgeProvider>
  </StrictMode>,
);
