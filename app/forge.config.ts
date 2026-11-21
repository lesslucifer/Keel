// Electron Forge: Vite builds the main part, the preload and the window; the packager makes Keel.app
// and signs it (KEEL_SIGN_IDENTITY, ad-hoc by default: the plan's signing ladder); the fuses plugin
// flips the fuses before the signature; the zip maker wraps it for darwin. Every value comes from
// src/build/package-options.ts, which a node test pins.

import type { ForgeConfig } from '@electron-forge/shared-types';
import { MakerZIP } from '@electron-forge/maker-zip';
import { VitePlugin } from '@electron-forge/plugin-vite';
import { FusesPlugin } from '@electron-forge/plugin-fuses';
import { FuseV1Options, FuseVersion, type FuseV1Config } from '@electron/fuses';
import { FUSES, PACKAGER, signOptions, perFileSign, type FuseName } from './src/build/package-options.ts';

const sign = signOptions(process.env);

// Where the packager keeps Electron's download. @electron/get no longer reads ELECTRON_CACHE itself,
// so it is passed on here: a sandboxed build sets it under $TMPDIR; unset, the user's cache folder.
const cacheRoot = process.env.ELECTRON_CACHE?.trim();

const fuses: FuseV1Config = { version: FuseVersion.V1 };
for (const [name, on] of Object.entries(FUSES) as Array<[FuseName, boolean]>) fuses[FuseV1Options[name]] = on;

const config: ForgeConfig = {
  packagerConfig: {
    name: PACKAGER.name,
    executableName: PACKAGER.executableName,
    appBundleId: PACKAGER.appBundleId,
    asar: PACKAGER.asar,
    protocols: PACKAGER.protocols.map((p) => ({ name: p.name, schemes: [...p.schemes] })),
    ...(cacheRoot ? { download: { cacheRoot } } : {}),
    // No osxNotarize: notarizing needs Apple's paid membership, rung 3, the owner's decision.
    osxSign: {
      identity: sign.identity,
      identityValidation: sign.identityValidation,
      optionsForFile: (file) => perFileSign(file, sign),
    },
  },
  makers: [new MakerZIP({}, ['darwin'])],
  plugins: [
    new VitePlugin({
      build: [
        { entry: { main: 'src/main/index.ts' }, config: 'vite.main.config.ts', target: 'main' },
        { entry: { preload: 'src/preload/index.ts' }, config: 'vite.preload.config.ts', target: 'preload' },
      ],
      renderer: [{ name: 'main_window', config: 'vite.renderer.config.ts' }],
    }),
    new FusesPlugin(fuses),
  ],
};

export default config;
