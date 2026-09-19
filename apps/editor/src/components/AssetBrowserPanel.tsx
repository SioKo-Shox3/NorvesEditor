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

/**
 * パネルを離れても残す絞り込み。store には載せない — 表示だけの状態で他パネルへ配る必要が
 * 無く、1 文字ごとの dispatch で全パネルが再描画される。dockview はタブを離れるとパネルを
 * アンマウントするので、モジュールスコープに置いて次に開いたときの初期値にする。
 * セッション内だけの記憶で、永続化はしない。
 */
let rememberedFilter = '';

/** テスト用: パネルをまたいで残る絞り込みを初期化する。 */
export function __resetAssetBrowserMemory(): void {
  rememberedFilter = '';
}

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
  // Panel-local, like the Outliner's: display-only state that would otherwise
  // re-render every panel through the shared context on each keystroke.
  // 絞り込みはパネルローカルで、パネルを離れても残す（Scene Outliner と同じ理由・同じ作り）。
  const [filter, setFilterState] = useState(rememberedFilter);
  function setFilter(next: string): void {
    rememberedFilter = next;
    setFilterState(next);
  }
  const visibleAssets = useMemo(() => filterAssets(assets, filter), [assets, filter]);
  // Grouping happens AFTER filtering, so a kind whose entries all dropped out
  // leaves no empty heading behind.
  const groupedAssets = useMemo(() => groupAssetsByKind(visibleAssets), [visibleAssets]);
  const selectedAssetKey = state.selectedAssetKey;

  // 上下で一覧を歩く（Outliner と同じ約束）。選択を動かし、焦点も移した行へ移す —
  // 焦点が置き去りになると次の矢印が効かない。端では止まる（回り込まない）。
  const listRef = useRef<HTMLDivElement>(null);

  function focusAssetRow(key: string): void {
    // セレクタを組み立てない。キーは logicalPath 由来で記号を含む。
    const rows = listRef.current?.querySelectorAll<HTMLButtonElement>(
      'button.scene-node__row[data-asset-key]',
    ) ?? [];
    for (const row of rows) {
      if (row.dataset.assetKey === key) {
        row.focus();
        return;
      }
    }
  }

  function handleListKeyDown(event: React.KeyboardEvent<HTMLDivElement>): void {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') {
      return;
    }
    const keys = flattenAssetKeys(groupedAssets);
    if (keys.length === 0) {
      return;
    }
    // 端でも既定動作は止める。止めないと一覧が裏でスクロールして、選択は動いていないのに
    // 画面だけ動く。
    event.preventDefault();

    const active = document.activeElement as HTMLElement | null;
    const focusedKey = active?.closest<HTMLElement>('[data-asset-key]')?.dataset.assetKey
      ?? selectedAssetKey;
    const index = focusedKey === undefined ? -1 : keys.indexOf(focusedKey);
    if (index < 0) {
      if (event.key === 'ArrowDown') {
        actions.selectAsset(keys[0]);
        focusAssetRow(keys[0]);
      }
      return;
    }
    const next = event.key === 'ArrowDown' ? index + 1 : index - 1;
    if (next < 0 || next >= keys.length) {
      return;
    }
    actions.selectAsset(keys[next]);
    focusAssetRow(keys[next]);
  }
  const hasManifest = state.assetManifest !== undefined;
  const isConnected = state.connection.status === 'connected';
  const canReloadRuntime =
    isConnected &&
    state.connection.capabilityNames?.has('asset.reload') === true &&
    !state.assetReloadUnsupported;
  // Show offline manifest read/parse errors whether or not a manifest is
  // already loaded, so a failed file refresh cannot silently keep a stale list.
  // Uses the dedicated assetError field, never the shared lastError.
  const assetError = state.assetError;
  const assetReloadError = state.assetReloadError;

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
            aria-label="Reload runtime asset manifest"
            disabled={!canReloadRuntime}
            onClick={() => { void actions.reloadAssetRuntime(); }}
          >
            Reload Runtime
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

        {hasManifest && assets.length > 0 && (
          <div className="panel__filter">
            <input
              type="search"
              aria-label="アセットを絞り込む"
              placeholder="パス / 種別 / variant で絞り込む"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>
        )}

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
              aria-label="Dismiss offline asset manifest error"
              onClick={actions.dismissAssetError}
            >
              x
            </button>
          </div>
        )}

        {assetReloadError !== undefined && (
          <div className="error-banner" role="alert">
            <span className="error-banner__kind">{assetReloadError.kind ?? 'asset'}</span>
            <span className="error-banner__message">
              {': '}
              {assetReloadError.message}
            </span>
            <button
              className="error-banner__dismiss"
              type="button"
              aria-label="Dismiss runtime asset manifest reload error"
              onClick={actions.dismissAssetReloadError}
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
        ) : visibleAssets.length === 0 ? (
          /* Filtered down to nothing — the manifest is fine, the query is not */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">一致なし</span>
            <span>一致するアセットがありません。</span>
            <span style={{ fontSize: 11 }}>No asset matches the filter.</span>
          </div>
        ) : (
          <div className="col" ref={listRef} onKeyDown={handleListKeyDown}>
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
                          // 矢印キーが「いまどの行に居るか」を読み、移った先へ焦点を移す目印。
                          data-asset-key={key}
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

/**
 * Narrow the asset list to the entries matching `query` (case-insensitive
 * substring over logical path, kind and variant).
 *
 * Not a regular expression, for the same reason as the Outliner's filter: a
 * stray metacharacter would silently empty the panel instead of narrowing it.
 * A blank query returns the input array itself, so an unfiltered panel does no
 * work and keeps its memoized grouping.
 */
export function filterAssets(assets: AssetEntry[], query: string): AssetEntry[] {
  const needle = query.trim().toLowerCase();
  if (needle === '') {
    return assets;
  }
  return assets.filter(
    (asset) =>
      asset.logicalPath.toLowerCase().includes(needle) ||
      asset.kind.toLowerCase().includes(needle) ||
      (asset.variant ?? '').toLowerCase().includes(needle),
  );
}

/**
 * 種別ごとに並べた一覧を、画面に見えている順（上から下）のキー列にする。
 * 矢印キーはこの並びの上だけを歩く。見出しは飛ばす（選べないので止まる場所にしない）。
 *
 * @param grouped `groupAssetsByKind` の結果
 * @returns 画面順のアセットキー
 */
export function flattenAssetKeys(grouped: Array<[string, AssetEntry[]]>): string[] {
  return grouped.flatMap(([, entries]) => entries.map((entry) => assetKeyForEntry(entry)));
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
