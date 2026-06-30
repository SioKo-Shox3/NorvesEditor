/**
 * AssetInspectorPanel — read-only detail view for an offline manifest entry.
 */

import { useEffect } from 'react';
import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import type { AssetEntry, AssetResolveResult } from '@norves/bridge-ui';
import { useBridgeState } from '../state/BridgeContext.js';
import { useBridgeActions } from '../hooks/useBridge.js';
import { assetKeyForEntry, findAssetEntryByKey } from '../state/store.js';

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function AssetInspectorPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useBridgeState();
  const { resolveAsset } = useBridgeActions();
  const selectedEntry = findAssetEntryByKey(state.assetManifest, state.selectedAssetKey);
  const selectedKey =
    selectedEntry !== undefined ? assetKeyForEntry(selectedEntry) : undefined;
  const isConnected = state.connection.status === 'connected';
  const assetResolveAvailable = state.assetCapabilitySupported !== false;
  const selectedResolve =
    selectedKey !== undefined ? state.assetResolveByKey?.[selectedKey] : undefined;

  useEffect(() => {
    if (
      isConnected
      && assetResolveAvailable
      && selectedEntry !== undefined
      && selectedResolve === undefined
    ) {
      void resolveAsset(selectedEntry.logicalPath, selectedEntry.kind, selectedEntry.variant);
    }
  }, [
    isConnected,
    assetResolveAvailable,
    selectedEntry,
    selectedResolve,
    resolveAsset,
  ]);

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Asset Inspector</span>
      </div>

      <div className="panel__body col">
        {state.selectedAssetKey === undefined ? (
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">選択なし</span>
            <span>Asset Browser でアセットを選択してください。</span>
          </div>
        ) : selectedEntry === undefined ? (
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">アセットが見つかりません</span>
            <span>選択中のアセットは現在の manifest にありません。</span>
          </div>
        ) : (
          <AssetFields
            entry={selectedEntry}
            health={buildHealthRows({
              isConnected,
              capabilitySupported: state.assetCapabilitySupported,
              resolve: selectedResolve,
              resolveErrorMessage:
                selectedKey !== undefined
                  ? state.assetResolveErrorByKey?.[selectedKey]?.message
                  : undefined,
            })}
          />
        )}
      </div>
    </div>
  );
}

interface HealthRowsInput {
  isConnected: boolean;
  capabilitySupported: boolean | undefined;
  resolve: AssetResolveResult | undefined;
  /** Per-key live resolve failure for the selected asset (not the manifest banner). */
  resolveErrorMessage: string | undefined;
}

function buildHealthRows(input: HealthRowsInput): Array<[string, string]> {
  if (!input.isConnected) {
    return [['health', '未検証(未接続)']];
  }
  if (input.capabilitySupported === false) {
    return [['health', '未対応']];
  }
  if (input.resolve !== undefined) {
    const rows: Array<[string, string]> = [
      ['health', statusLabel(input.resolve.status)],
      ['status', input.resolve.status],
      ['source', input.resolve.source],
      [
        'requiresExplicitLog',
        // Optional on the wire: distinguish "engine omitted it" from an explicit false.
        input.resolve.requiresExplicitLog === undefined
          ? '未設定'
          : String(input.resolve.requiresExplicitLog),
      ],
    ];
    if (input.resolve.reason !== undefined && input.resolve.reason !== '') {
      rows.push(['reason', input.resolve.reason]);
    }
    return rows;
  }
  if (input.resolveErrorMessage !== undefined) {
    return [
      ['health', '未確定'],
      ['reason', input.resolveErrorMessage],
    ];
  }
  return [['health', '確認中']];
}

function AssetFields({
  entry,
  health,
}: {
  entry: AssetEntry;
  health: Array<[string, string]>;
}): React.JSX.Element {
  const fields: Array<[string, string | number | undefined]> = [
    ['logicalPath', entry.logicalPath],
    ['kind', entry.kind],
    ['variant', entry.variant],
    ['format', entry.format],
    ['sourceHash', entry.sourceHash],
    ['cookedPackage', entry.cookedPackage],
    ['entryName', entry.entryName],
    ['entryType', entry.entryType],
    ['cookedHash', entry.cookedHash],
    ['cookedVersion', entry.cookedVersion],
  ];

  return (
    <div className="inspector">
      <div className="inspector__header">
        <span className="inspector__name">{entry.logicalPath}</span>
        <span className="inspector__kind">{entry.kind}</span>
        <span className="inspector__id">{entry.variant ?? 'default'}</span>
      </div>
      <ul className="inspector__props">
        {health.map(([name, value]) => (
          <li className="inspector-prop" key={`health:${name}`}>
            <span className="inspector-prop__name">{name}</span>
            <span className="inspector-prop__value">{value}</span>
            <span className="inspector-prop__type">live</span>
          </li>
        ))}
        {fields.map(([name, value]) => (
          <li className="inspector-prop" key={name}>
            <span className="inspector-prop__name">{name}</span>
            <span className="inspector-prop__value">{formatFieldValue(value)}</span>
            <span className="inspector-prop__type">read-only</span>
          </li>
        ))}
      </ul>
    </div>
  );
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

function formatFieldValue(value: string | number | undefined): string {
  if (value === undefined || value === '') {
    return '未設定';
  }
  return String(value);
}
