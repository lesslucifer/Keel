// The window's entry: React renders the bare page into #root. W8 replaces the page with screen 6.
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App.tsx';

const root = document.getElementById('root');
if (root === null) throw new Error('the page has no #root');
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
