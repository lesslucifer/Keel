import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PACKAGER, FUSES, AD_HOC, signOptions, MAIN_ENTITLEMENTS, perFileSign } from '../../src/build/package-options.ts';

test('the bundle is Keel, id dev.keel.app (it keys the notification permission), an asar, and declares the keel link scheme', () => {
  assert.deepEqual(JSON.parse(JSON.stringify(PACKAGER)), {
    name: 'Keel',
    executableName: 'Keel',
    appBundleId: 'dev.keel.app',
    asar: true,
    protocols: [{ name: 'Keel', schemes: ['keel'] }],
  });
  assert.ok(Object.isFrozen(PACKAGER));
});

test('the fuses are the plan\'s seven: Node\'s ways in off, cookies encrypted, the asar checked and the only app, file:// unprivileged', () => {
  assert.deepEqual(JSON.parse(JSON.stringify(FUSES)), {
    RunAsNode: false,
    EnableNodeOptionsEnvironmentVariable: false,
    EnableNodeCliInspectArguments: false,
    EnableCookieEncryption: true,
    EnableEmbeddedAsarIntegrityValidation: true,
    OnlyLoadAppFromAsar: true,
    GrantFileProtocolExtraPrivileges: false,
  });
  assert.ok(Object.isFrozen(FUSES));
});

test('signing is ad-hoc ("-") when KEEL_SIGN_IDENTITY is unset, blank or "-", with no keychain lookup and no hardened runtime', () => {
  assert.equal(AD_HOC, '-');
  for (const v of [undefined, '', '   ', '-', ' - ']) {
    assert.deepEqual(signOptions({ KEEL_SIGN_IDENTITY: v }), { identity: '-', identityValidation: false, hardenedRuntime: false }, String(v));
  }
});

test('a named identity is trimmed, looked up in the keychain, and signed with the hardened runtime', () => {
  assert.deepEqual(signOptions({ KEEL_SIGN_IDENTITY: '  Apple Development: Me (ABCDE12345) ' }), {
    identity: 'Apple Development: Me (ABCDE12345)',
    identityValidation: true,
    hardenedRuntime: true,
  });
});

test('an identity with a control character is refused', () => {
  assert.throws(() => signOptions({ KEEL_SIGN_IDENTITY: 'Apple Development: Me\n--deep' }), /control/);
});

test('the app bundle itself gets only the JIT entitlement; helpers and frameworks keep the signer\'s own; each call a fresh list', () => {
  assert.deepEqual(MAIN_ENTITLEMENTS, ['com.apple.security.cs.allow-jit']);
  const adHoc = signOptions({});
  const app = perFileSign('/out/Keel-darwin-arm64/Keel.app', adHoc);
  assert.deepEqual(app, { hardenedRuntime: false, entitlements: ['com.apple.security.cs.allow-jit'] });
  assert.notEqual(app.entitlements, perFileSign('/out/Keel-darwin-arm64/Keel.app', adHoc).entitlements);
  for (const inner of ['/out/Keel.app/Contents/Frameworks/Keel Helper (Renderer).app', '/out/Keel.app/Contents/Frameworks/Electron Framework.framework']) {
    assert.deepEqual(perFileSign(inner, adHoc), { hardenedRuntime: false }, inner);
  }
  assert.deepEqual(perFileSign('/out/Keel.app', signOptions({ KEEL_SIGN_IDENTITY: 'Apple Development: Me' })).hardenedRuntime, true);
});
