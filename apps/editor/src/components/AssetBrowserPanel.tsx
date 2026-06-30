/**
 * AssetBrowserPanel — offline manifest browser (Phase B).
 *
 * Reads an explicit manifest.json path through the Rust backend. The list is
 * editor-local static file data, independent of Bridge connection state.
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import type { AssetEntry, AssetResolveResult } from '@norves/bridge-ui';
import { useBridgeState } from '../state/BridgeContext.js';
import { useBridgeActions } from '../hooks/useBridge.js';
import { assetKeyForEntry } from '../state/store.js';

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function AssetBrowserPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useBridgeState();
  const actions = useBridgeActions();

  const defaultPath = defaultManifestPath(state.workspace?.rootPath);
  const previousDefaultPathRef = useRef(defaultPath);
  const [manifestPath, setManifestPath] = useState(defaultPath);

  useEffect(() => {
    const previousDefault = previousDefaultPathRef.current;
    previousDefaultPathRef.current = defaultPath;
    setManifestPath((current) => {
      if (current.trim() === '' || current === previousDefault) {
        return defaultPath;
      }
      return current;
    });
  }, [defaultPath]);

  const assets = state.assetManifest?.assets ?? [];
  const groupedAssets = useMemo(() => groupAssetsByKind(assets), [assets]);
  const selectedAssetKey = state.selectedAssetKey;
  const hasManifest = state.assetManifest !== undefined;
  const isConnected = state.connection.status === 'connected';
  // Show asset errors whether or not a manifest is already loaded, so a failed
  // *reload* is visible instead of silently keeping the stale list. Uses the
  // dedicated assetError field, never the shared lastError.
  const assetError = state.assetError;

  const handleLoad = (): void => {
    const trimmed = manifestPath.trim();
    if (trimmed.length > 0) {
      void actions.readAssetManifest(trimmed);
    }
  };

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Asset Browser</span>
      </div>

      <div className="panel__body col">
        <div className="row" style={{ alignItems: 'stretch' }}>
          <input
            className="input"
            type="text"
            value={manifestPath}
            placeholder="manifest.json path"
            aria-label="Manifest path"
            onChange={(event) => setManifestPath(event.target.value)}
          />
          <button
            className="btn btn--primary"
            type="button"
            disabled={manifestPath.trim().length === 0}
            onClick={handleLoad}
          >
            Load
          </button>
          <button
            className="btn"
            type="button"
            disabled={!hasManifest}
            onClick={actions.clearAssetManifest}
          >
            Clear
          </button>
        </div>

        {assetError !== undefined && (
          <div className="error-banner" role="alert">
            <span className="error-banner__kind">{assetError.kind ?? 'asset'}</span>
            <span className="error-banner__message">
              {': '}
              {assetError.message}
            </span>
            <button
              className="error-banner__dismiss"
              type="button"
              aria-label="Dismiss error"
              onClick={actions.dismissAssetError}
            >
              x
            </button>
          </div>
        )}

        {!hasManifest ? (
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">manifest 未ロード</span>
            <span>manifest.json を指定して Load してください。</span>
          </div>
        ) : assets.length === 0 ? (
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">アセット 0 件</span>
            <span>この manifest にはアセットがありません。</span>
          </div>
        ) : (
          <div className="col">
            {groupedAssets.map(([kind, entries]) => (
              <section className="col" key={kind}>
                <div className="label">{kind}</div>
                <ul className="scene-tree">
                  {entries.map((entry) => {
                    const key = assetKeyForEntry(entry);
                    const selected = key === selectedAssetKey;
                    const resolve = selected ? state.assetResolveByKey?.[key] : undefined;
                    return (
                      <li className="scene-node" key={key}>
                        <button
                          type="button"
                          className={`scene-node__row${selected ? ' scene-node__row--selected' : ''}`}
                          aria-selected={selected}
                          onClick={() => actions.selectAsset(key)}
                        >
                          <span className="scene-node__name">{entry.logicalPath}</span>
                          <span className="scene-node__kind">{entry.variant ?? 'default'}</span>
                          <span className="scene-node__kind">
                            {healthLabel({
                              isConnected,
                              capabilitySupported: state.assetCapabilitySupported,
                              selected,
                              resolve,
                              resolveErrorMessage: selected
                                ? state.assetResolveErrorByKey?.[key]?.message
                                : undefined,
                            })}
                          </span>
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </section>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function defaultManifestPath(rootPath: string | undefined): string {
  if (rootPath === undefined || rootPath.trim() === '') {
    return '';
  }
  return `${rootPath.replace(/[\\/]+$/, '')}/manifest.json`;
}

function groupAssetsByKind(assets: AssetEntry[]): Array<[string, AssetEntry[]]> {
  const groups = new Map<string, AssetEntry[]>();
  for (const asset of assets) {
    const entries = groups.get(asset.kind);
    if (entries === undefined) {
      groups.set(asset.kind, [asset]);
    } else {
      entries.push(asset);
    }
  }
  return Array.from(groups.entries()).sort(([left], [right]) => left.localeCompare(right));
}

interface HealthLabelInput {
  isConnected: boolean;
  capabilitySupported: boolean | undefined;
  selected: boolean;
  resolve: AssetResolveResult | undefined;
  /** Per-key live resolve failure for this row (not the manifest banner). */
  resolveErrorMessage: string | undefined;
}

function healthLabel(input: HealthLabelInput): string {
  if (!input.isConnected) {
    return '未検証(未接続)';
  }
  if (input.capabilitySupported === false) {
    return '未対応';
  }
  if (!input.selected) {
    return '未検証';
  }
  if (input.resolve !== undefined) {
    return statusLabel(input.resolve.status);
  }
  if (input.resolveErrorMessage !== undefined) {
    return '未確定';
  }
  return '確認中';
}

function statusLabel(status: AssetResolveResult['status']): string {
  switch (status) {
    case 'successCooked':
      return 'cooked OK';
    case 'successLoose':
      return 'loose';
    case 'cookedEntryHashMismatch':
      return 'hash mismatch';
    case 'invalidRequest':
      return 'invalid request';
    case 'invalidManifest':
      return 'invalid manifest';
    case 'looseReadFailed':
      return 'loose read failed';
    case 'cookedPackageReadFailed':
      return 'package read failed';
    case 'cookedPackageParseFailed':
      return 'package parse failed';
    case 'cookedEntryMissing':
      return 'entry missing';
    default: {
      const _exhaustive: never = status;
      return _exhaustive;
    }
  }
}
