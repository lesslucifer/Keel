// What forge.config.ts packages with, as plain data so a node test pins it with no package
// installed: the bundle's name and id, the keel link scheme (declared now, handled in W5), the fuses
// by name, and the signing rung read from KEEL_SIGN_IDENTITY (ad-hoc unless the owner names a
// certificate: the plan's signing ladder).

/** The packager's fixed options. The bundle id keys macOS's notification permission: keep it. */
export const PACKAGER = Object.freeze({
  name: 'Keel',
  executableName: 'Keel',
  appBundleId: 'dev.keel.app',
  asar: true,
  protocols: Object.freeze([Object.freeze({ name: 'Keel', schemes: Object.freeze(['keel']) })]),
});

/** The fuses, by their FuseV1Options names: Node's ways in off, the asar checked and the only app, file:// unprivileged. */
export const FUSES = Object.freeze({
  RunAsNode: false,
  EnableNodeOptionsEnvironmentVariable: false,
  EnableNodeCliInspectArguments: false,
  EnableCookieEncryption: true,
  EnableEmbeddedAsarIntegrityValidation: true,
  OnlyLoadAppFromAsar: true,
  GrantFileProtocolExtraPrivileges: false,
});

export type FuseName = keyof typeof FUSES;

/** codesign's ad-hoc identity: no certificate, valid on the Mac that built it (rung 1). */
export const AD_HOC = '-';

export interface SignChoice {
  identity: string;
  /** Whether the signer looks the identity up in the keychain first (an ad-hoc one has nothing to find). */
  identityValidation: boolean;
  /** The hardened runtime: on for a named certificate; off for ad-hoc, which has no team to bind it to. */
  hardenedRuntime: boolean;
}

const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** The signing rung: KEEL_SIGN_IDENTITY trimmed, or ad-hoc when it is unset, blank or "-". */
export function signOptions(env: Record<string, string | undefined>): SignChoice {
  const raw = env.KEEL_SIGN_IDENTITY;
  const id = typeof raw === 'string' ? raw.trim() : '';
  if (CONTROL.test(id)) throw new Error('KEEL_SIGN_IDENTITY holds a control character');
  if (id === '' || id === AD_HOC) return { identity: AD_HOC, identityValidation: false, hardenedRuntime: false };
  return { identity: id, identityValidation: true, hardenedRuntime: true };
}

/** The app bundle's own entitlements: V8's JIT only. No camera, microphone, location or other device. */
export const MAIN_ENTITLEMENTS: readonly string[] = Object.freeze(['com.apple.security.cs.allow-jit']);

/**
 * The signer's per-file options: the hardened runtime by the rung; the app bundle itself gets only
 * MAIN_ENTITLEMENTS, and the helpers and frameworks inside it keep the signer's own per-helper set.
 * A fresh list each call: the signer may replace a list it is given.
 */
export function perFileSign(file: string, choice: SignChoice): { hardenedRuntime: boolean; entitlements?: string[] } {
  const isOuterApp = file.endsWith('.app') && !file.slice(0, -'.app'.length).includes('.app/');
  return isOuterApp ? { hardenedRuntime: choice.hardenedRuntime, entitlements: [...MAIN_ENTITLEMENTS] } : { hardenedRuntime: choice.hardenedRuntime };
}
