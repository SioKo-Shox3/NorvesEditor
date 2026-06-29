/**
 * AssetInspectorPanel — read-only detail view for an offline manifest entry.
 */

import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import type { AssetEntry } from '@norves/bridge-ui';
import { useBridgeState } from '../state/BridgeContext.js';
import { findAssetEntryByKey } from '../state/store.js';

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function AssetInspectorPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useBridgeState();
  const selectedEntry = findAssetEntryByKey(state.assetManifest, state.selectedAssetKey);

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
          <AssetFields entry={selectedEntry} />
        )}
      </div>
    </div>
  );
}

function AssetFields({ entry }: { entry: AssetEntry }): React.JSX.Element {
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

function formatFieldValue(value: string | number | undefined): string {
  if (value === undefined || value === '') {
    return '未設定';
  }
  return String(value);
}
