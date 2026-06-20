/**
 * PropertyInspectorPanel — read-only property inspector (Phase 4:
 * object.getSnapshot + schema.getSnapshot wired).
 *
 * Shows the selected object's properties as a read-only list. The object
 * snapshot is fetched whenever selectedObjectId changes; the type schema is
 * fetched once on (re)connect and used as an auxiliary hint for property
 * valueTypes. There is NO write path here (Phase 5 adds editing) — the value
 * renderer is, however, split by value kind so Phase 5 can swap a display cell
 * for an edit control per type without restructuring.
 *
 * Selection -> fetch race guard:
 *  - An effect keyed on selectedObjectId calls getObjectSnapshot(id) on change.
 *  - The store clears objectSnapshot on selection change, so a stale snapshot is
 *    never shown for a newer selection. We additionally guard the displayed
 *    snapshot: it is only rendered when its objectId matches the current
 *    selection, so an out-of-order late response for a previous object is
 *    discarded at render time.
 *
 * Engine-agnostic degradation (no mock-specific assumptions):
 *  (a) disconnected             → "エンジンに接続するとプロパティが表示されます"
 *  (b) connected, no selection  → "選択なし"
 *  (c) METHOD_NOT_SUPPORTED      → "この engine はオブジェクト照会に未対応"
 *      (store.objectUnsupported, set when object/schema query answers
 *       METHOD_NOT_SUPPORTED — works for any engine, not just the mock).
 *  (d) empty property bag        → "プロパティがありません"
 */

import { useEffect, useRef } from 'react';
import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import type { ObjectSnapshot, PropertyEntry, PropertyValue, TypeDescriptor } from '@norves/bridge-ui';
import { useBridgeState } from '../state/BridgeContext.js';
import { useBridgeActions } from '../hooks/useBridge.js';

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function PropertyInspectorPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useBridgeState();
  const actions = useBridgeActions();

  const isConnected = state.connection.status === 'connected';
  const selectedObjectId = state.selectedObjectId;
  const objectSnapshot = state.objectSnapshot;
  const schemaTypes = state.schemaTypes;
  const objectUnsupported = state.objectUnsupported === true;

  // -----------------------------------------------------------------------
  // Fetch the type schema once each time we (re)enter the connected state.
  // A ref tracks the previous connection status so we only fetch on the
  // disconnected/connecting -> connected edge. The store clears schemaTypes on
  // disconnect so this re-probes a fresh engine.
  // -----------------------------------------------------------------------
  const wasConnectedRef = useRef(false);
  useEffect(() => {
    if (isConnected && !wasConnectedRef.current) {
      void actions.getSchemaSnapshot();
    }
    wasConnectedRef.current = isConnected;
  }, [isConnected, actions]);

  // -----------------------------------------------------------------------
  // Fetch the selected object's snapshot whenever the selection changes.
  // Keyed on selectedObjectId so only the latest selection triggers a fetch;
  // the store already drops the prior snapshot on selection change, and the
  // render guard below discards any late response whose objectId no longer
  // matches the current selection.
  // -----------------------------------------------------------------------
  const getObjectSnapshot = actions.getObjectSnapshot;
  useEffect(() => {
    if (isConnected && selectedObjectId !== undefined) {
      void getObjectSnapshot(selectedObjectId);
    }
  }, [isConnected, selectedObjectId, getObjectSnapshot]);

  // The stored snapshot is only valid for the current selection. A late
  // response for a previous object (whose objectId differs) is discarded here.
  const currentSnapshot =
    objectSnapshot !== undefined && objectSnapshot.objectId === selectedObjectId
      ? objectSnapshot
      : undefined;

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Property Inspector</span>
      </div>

      <div className="panel__body col">
        {!isConnected ? (
          /* (a) Disconnected — engine not attached */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">Property Inspector</span>
            <span>エンジンに接続するとプロパティが表示されます。</span>
            <span style={{ fontSize: 11 }}>Connect to an engine to inspect properties.</span>
          </div>
        ) : objectUnsupported ? (
          /* (c) Engine does not implement object query (METHOD_NOT_SUPPORTED) */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">オブジェクト照会に未対応</span>
            <span>この engine はオブジェクト照会に未対応です。</span>
            <span style={{ fontSize: 11 }}>This engine does not support object inspection.</span>
          </div>
        ) : selectedObjectId === undefined ? (
          /* (b) Connected but nothing selected */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">選択なし</span>
            <span>Scene Outliner でオブジェクトを選択してください。</span>
            <span style={{ fontSize: 11 }}>
              Select an object in the Scene Outliner to inspect its properties.
            </span>
          </div>
        ) : currentSnapshot === undefined ? (
          /* Selected but the snapshot for THIS object has not arrived yet */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">{selectedObjectId}</span>
            <span>プロパティを読み込み中...</span>
            <span style={{ fontSize: 11 }}>Loading properties...</span>
          </div>
        ) : currentSnapshot.properties.length === 0 ? (
          /* (d) Empty property bag */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">{snapshotTitle(currentSnapshot)}</span>
            <span>プロパティがありません。</span>
            <span style={{ fontSize: 11 }}>This object has no properties.</span>
          </div>
        ) : (
          <ObjectProperties snapshot={currentSnapshot} schemaTypes={schemaTypes} />
        )}
      </div>
    </div>
  );
}

/** Header label for an object: prefer name, fall back to objectId. */
function snapshotTitle(snapshot: ObjectSnapshot): string {
  return snapshot.name ?? snapshot.objectId;
}

// -------------------------------------------------------------------------
// Object header + property table
// -------------------------------------------------------------------------

interface ObjectPropertiesProps {
  snapshot: ObjectSnapshot;
  schemaTypes: TypeDescriptor[] | undefined;
}

function ObjectProperties({ snapshot, schemaTypes }: ObjectPropertiesProps): React.JSX.Element {
  // Auxiliary schema hint: the descriptor whose typeName matches the object's
  // kind, if any. Used to fill in a property's valueType when the snapshot entry
  // omits it. Manual typeName<->kind matching is an alpha simplification.
  const descriptor =
    snapshot.kind !== undefined
      ? schemaTypes?.find((t) => t.typeName === snapshot.kind)
      : undefined;

  function valueTypeFor(entry: PropertyEntry): string | undefined {
    if (entry.valueType !== undefined) return entry.valueType;
    return descriptor?.properties?.find((p) => p.name === entry.name)?.valueType;
  }

  return (
    <div className="inspector">
      <div className="inspector__header">
        <span className="inspector__name">{snapshotTitle(snapshot)}</span>
        {snapshot.kind !== undefined && (
          <span className="inspector__kind">{snapshot.kind}</span>
        )}
        <span className="inspector__id">{snapshot.objectId}</span>
      </div>

      <ul className="inspector__props">
        {snapshot.properties.map((entry) => (
          <li className="inspector-prop" key={entry.name}>
            <span className="inspector-prop__name">{entry.name}</span>
            <span className="inspector-prop__value">
              <PropertyValueView value={entry.value} />
            </span>
            {valueTypeFor(entry) !== undefined && (
              <span className="inspector-prop__type">{valueTypeFor(entry)}</span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

// -------------------------------------------------------------------------
// Type-driven value renderer (Phase 5 will swap display cells for editors)
// -------------------------------------------------------------------------

/** Coarse classification of a PropertyValue for type-driven rendering. */
type ValueKind = 'string' | 'number' | 'boolean' | 'null' | 'array' | 'object';

function classifyValue(value: PropertyValue): ValueKind {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  switch (typeof value) {
    case 'string':
      return 'string';
    case 'number':
      return 'number';
    case 'boolean':
      return 'boolean';
    default:
      return 'object';
  }
}

interface PropertyValueViewProps {
  value: PropertyValue;
}

/**
 * Renders a property value branched by kind. Scalars render inline; array /
 * object values render a readable JSON preview inside a <details> so deep
 * structures are collapsible (no unbounded recursion: JSON.stringify is a
 * single, depth-safe serialization). Each branch is an explicit cell so Phase 5
 * can replace a display cell with an edit control per kind.
 */
function PropertyValueView({ value }: PropertyValueViewProps): React.JSX.Element {
  const kind = classifyValue(value);

  switch (kind) {
    case 'null':
      return <span className="value value--null">null</span>;
    case 'string':
      return <span className="value value--string">{value as string}</span>;
    case 'number':
      return <span className="value value--number">{String(value as number)}</span>;
    case 'boolean':
      return <span className="value value--boolean">{(value as boolean) ? 'true' : 'false'}</span>;
    case 'array':
    case 'object': {
      // Compact summary on the row + collapsible full JSON preview. JSON.stringify
      // serializes the whole structure once (depth-safe), avoiding any per-node
      // recursive React tree for arbitrarily nested values.
      const isArray = kind === 'array';
      const summary = isArray
        ? `Array(${(value as PropertyValue[]).length})`
        : `Object{${Object.keys(value as Record<string, PropertyValue>).length}}`;
      const preview = JSON.stringify(value, null, 2);
      return (
        <details className={`value value--${kind}`}>
          <summary className="value__summary">{summary}</summary>
          <pre className="value__json">{preview}</pre>
        </details>
      );
    }
    default: {
      // Exhaustiveness guard — TypeScript catches any unhandled kind.
      const _exhaustive: never = kind;
      return <span>{String(_exhaustive)}</span>;
    }
  }
}
