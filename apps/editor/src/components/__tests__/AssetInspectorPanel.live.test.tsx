// @vitest-environment jsdom
/**
 * AssetInspectorPanel live integration tests.
 *
 * Uses the real BridgeProvider/useBridgeActions path so the asset.resolve race
 * guard is exercised through the component effect and reducer.
 */

import { describe, it, expect, vi, afterEach, beforeEach, type Mock } from 'vitest';
import React from 'react';
import { render, screen, cleanup, act, waitFor } from '@testing-library/react';
import type { AssetManifestPayload, AssetResolveResult } from '@norves/bridge-ui';
import type { BridgeAction } from '../../state/store.js';
import { assetKeyForEntry } from '../../state/store.js';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import * as tauriCore from '@tauri-apps/api/core';
import { BridgeProvider, useBridgeDispatch } from '../../state/BridgeContext.js';
import { AssetInspectorPanel } from '../AssetInspectorPanel.js';

afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks();
});

const LIVE_MANIFEST: AssetManifestPayload = {
  version: 1,
  manifestPath: 'C:/Project/manifest.json',
  assets: [
    { logicalPath: 'textures/a.png', kind: 'texture', variant: 'default' },
    { logicalPath: 'textures/b.png', kind: 'texture', variant: 'default' },
  ],
};

function createDeferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function DispatchCapture({
  onDispatch,
}: {
  onDispatch: (dispatch: React.Dispatch<BridgeAction>) => void;
}): null {
  const dispatch = useBridgeDispatch();
  onDispatch(dispatch);
  return null;
}

function resolveResult(
  status: AssetResolveResult['status'],
  logicalPath: string,
): AssetResolveResult {
  return {
    status,
    source: status === 'successCooked' ? 'cooked' : 'none',
    normalizedLogicalPath: logicalPath,
  };
}

describe('AssetInspectorPanel — live resolve race guard', () => {
  it('discards a stale selected asset response after the user selects another asset', async () => {
    const deferredA = createDeferred<AssetResolveResult>();
    const deferredB = createDeferred<AssetResolveResult>();
    (tauriCore.invoke as Mock).mockImplementation(
      (command: string, args?: { logicalPath?: string }) => {
        if (command === 'asset_resolve' && args?.logicalPath === 'textures/a.png') {
          return deferredA.promise;
        }
        if (command === 'asset_resolve' && args?.logicalPath === 'textures/b.png') {
          return deferredB.promise;
        }
        return Promise.resolve(undefined);
      },
    );

    let dispatch: React.Dispatch<BridgeAction> | undefined;
    const dockviewProps = {} as React.ComponentProps<typeof AssetInspectorPanel>;
    render(
      <BridgeProvider>
        <DispatchCapture onDispatch={(next) => { dispatch = next; }} />
        <AssetInspectorPanel {...dockviewProps} />
      </BridgeProvider>,
    );

    const entryA = LIVE_MANIFEST.assets[0]!;
    const entryB = LIVE_MANIFEST.assets[1]!;
    const keyA = assetKeyForEntry(entryA);
    const keyB = assetKeyForEntry(entryB);

    await act(async () => {
      dispatch?.({ type: 'assetManifestLoaded', payload: LIVE_MANIFEST });
      dispatch?.({ type: 'connectionStateChanged', payload: { connected: true, sessionId: 's1' } });
      dispatch?.({ type: 'assetSelected', key: keyA });
    });

    await waitFor(() => {
      expect(tauriCore.invoke).toHaveBeenCalledWith('asset_resolve', {
        logicalPath: entryA.logicalPath,
        kind: entryA.kind,
        variant: entryA.variant,
      });
    });

    await act(async () => {
      dispatch?.({ type: 'assetSelected', key: keyB });
    });

    await waitFor(() => {
      expect(tauriCore.invoke).toHaveBeenCalledWith('asset_resolve', {
        logicalPath: entryB.logicalPath,
        kind: entryB.kind,
        variant: entryB.variant,
      });
    });

    await act(async () => {
      deferredA.resolve(resolveResult('successCooked', entryA.logicalPath));
      await Promise.resolve();
    });

    expect(screen.getAllByText(entryB.logicalPath).length).toBeGreaterThan(0);
    expect(screen.getByText('確認中')).toBeTruthy();
    expect(screen.queryByText('successCooked')).toBeNull();
    expect(screen.queryByText('cooked OK')).toBeNull();

    await act(async () => {
      deferredB.resolve(resolveResult('cookedEntryHashMismatch', entryB.logicalPath));
      await Promise.resolve();
    });

    expect(await screen.findByText('hash mismatch')).toBeTruthy();
  });
});
