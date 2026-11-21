// The preload: the window's one door. The sandbox is on, so this runs with no Node; Electron gives
// it contextBridge and ipcRenderer only. It exposes window.keel = makeBridge(ipcRenderer) - ask() on
// the request channel and on() for the contract's push events - and nothing else.

import { contextBridge, ipcRenderer } from 'electron';
import { makeBridge } from './bridge.ts';

contextBridge.exposeInMainWorld('keel', makeBridge(ipcRenderer));
