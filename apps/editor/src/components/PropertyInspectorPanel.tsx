/**
 * PropertyInspectorPanel — editable property inspector (Phase 5:
 * object.setProperty write path, on top of Phase 4 read paths).
 *
 * Shows the selected object's properties and lets the user edit them. The object
 * snapshot is fetched whenever selectedObjectId changes; the type schema is
 * fetched once on (re)connect and used as an auxiliary hint for property
 * valueTypes. Each property row carries an edit control chosen by value kind:
 *   string  → text input          (commit on blur / Enter)
 *   number  → number input        (commit on blur / Enter)
 *   boolean → checkbox            (commit immediately on toggle)
 *   null    → JSON editor         (so it can be set to any JSON value)
 *   array   → JSON editor + Apply (JSON.parse on commit; inline error if invalid)
 *   object  → JSON editor + Apply (JSON.parse on commit; inline error if invalid)
 * Value-kind classification reuses classifyValue (from Phase 4).
 *
 * Edit-state locality (Phase 1 review): the in-progress edit lives in
 * component-local state inside each PropertyEditor row. A keystroke never
 * dispatches, so the BridgeStateContext single-value broadcast does not
 * re-render every panel on every keystroke. We dispatch (via setObjectProperty)
 * ONLY on commit (blur / Enter for scalars, Apply for JSON, toggle for boolean).
 *
 * On a successful write the engine's appliedValue updates the store snapshot
 * (objectPropertyApplied). A rejected write (accepted:false) or a backend/engine
 * error is surfaced inline on the row; a pending write disables the row.
 *
 * Selection -> fetch race guard (unchanged from Phase 4):
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

import { useEffect, useRef, useState } from 'react';
import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import type {
  ObjectSnapshot,
  PropertyEntry,
  PropertyValue,
  TypeDescriptor,
} from '@norves/bridge-ui';
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
              {/*
                Key the editor by objectId + property + a serialization of the
                committed value so that when the store snapshot is replaced (a
                fresh fetch or an applied write) the editor re-seeds its local
                state from the new value instead of keeping stale local edits.
              */}
              <PropertyEditor
                key={`${snapshot.objectId}:${entry.name}:${stableValueKey(entry.value)}`}
                objectId={snapshot.objectId}
                property={entry.name}
                value={entry.value}
              />
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
// Type-driven value classification (shared by display + edit)
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

/** A stable string key for a committed value, used to reset editor local state. */
function stableValueKey(value: PropertyValue): string {
  return JSON.stringify(value) ?? 'undefined';
}

// -------------------------------------------------------------------------
// Per-property editor (component-local edit state; commit-only dispatch)
// -------------------------------------------------------------------------

interface PropertyEditorProps {
  objectId: string;
  property: string;
  /** The committed value from the store snapshot (the seed for local state). */
  value: PropertyValue;
}

/** Inline feedback shown under a row after a commit attempt. */
type RowFeedback =
  | { kind: 'none' }
  | { kind: 'invalidJson'; message: string }
  | { kind: 'rejected' }
  | { kind: 'error'; message: string };

/**
 * One editable property row. Holds the in-progress edit in LOCAL state so a
 * keystroke never dispatches (no per-keystroke全パネル re-render). Commits via
 * setObjectProperty only on blur / Enter (scalars), toggle (boolean), or Apply
 * (JSON for null / array / object). On a successful accepted write the store
 * snapshot is updated by the action; this row's key changes and it re-seeds.
 */
function PropertyEditor({ objectId, property, value }: PropertyEditorProps): React.JSX.Element {
  const actions = useBridgeActions();
  const kind = classifyValue(value);

  // Whether a commit is in flight (disables the control + shows a hint).
  const [pending, setPending] = useState(false);
  const [feedback, setFeedback] = useState<RowFeedback>({ kind: 'none' });

  // Submit a committed value to the engine. Centralizes the pending / feedback
  // lifecycle for every editor kind. The value here is already a real
  // PropertyValue (scalars are coerced before calling; JSON editors JSON.parse
  // before calling and report invalid JSON without ever calling commit).
  async function commit(next: PropertyValue): Promise<void> {
    setPending(true);
    setFeedback({ kind: 'none' });
    try {
      const result = await actions.setObjectProperty(objectId, property, next);
      if (!result.accepted) {
        setFeedback({ kind: 'rejected' });
      }
      // On accept the store snapshot updates and this row re-seeds via its key.
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      setFeedback({ kind: 'error', message });
    } finally {
      setPending(false);
    }
  }

  return (
    <span className="prop-editor">
      <PropertyEditorControl
        kind={kind}
        value={value}
        pending={pending}
        onCommitValue={(next) => void commit(next)}
        onInvalidJson={(message) => setFeedback({ kind: 'invalidJson', message })}
        onClearFeedback={() => setFeedback({ kind: 'none' })}
      />
      <RowFeedbackView feedback={feedback} pending={pending} />
    </span>
  );
}

interface PropertyEditorControlProps {
  kind: ValueKind;
  value: PropertyValue;
  pending: boolean;
  /** Commit a parsed/coerced PropertyValue to the engine. */
  onCommitValue: (next: PropertyValue) => void;
  /** Report a JSON parse failure (JSON editors only) for inline display. */
  onInvalidJson: (message: string) => void;
  /** Clear any prior inline feedback (e.g. when the user starts editing again). */
  onClearFeedback: () => void;
}

/** Picks the edit control by value kind. */
function PropertyEditorControl(props: PropertyEditorControlProps): React.JSX.Element {
  switch (props.kind) {
    case 'string':
      return <StringEditor {...props} />;
    case 'number':
      return <NumberEditor {...props} />;
    case 'boolean':
      return <BooleanEditor {...props} />;
    case 'null':
    case 'array':
    case 'object':
      // null / array / object all edit through the JSON editor so the user can
      // set any JSON value (a null can become a scalar, an array can be reshaped).
      return <JsonEditor {...props} />;
    default: {
      // Exhaustiveness guard — TypeScript catches any unhandled kind.
      const _exhaustive: never = props.kind;
      return <span>{String(_exhaustive)}</span>;
    }
  }
}

// ---- scalar editors -------------------------------------------------------

function StringEditor({
  value,
  pending,
  onCommitValue,
  onClearFeedback,
}: PropertyEditorControlProps): React.JSX.Element {
  const [draft, setDraft] = useState<string>(value as string);

  function commitIfChanged(): void {
    if (draft !== (value as string)) {
      onCommitValue(draft);
    }
  }

  return (
    <input
      className="value value--string prop-editor__input"
      type="text"
      value={draft}
      disabled={pending}
      onChange={(e) => {
        setDraft(e.target.value);
        onClearFeedback();
      }}
      onBlur={commitIfChanged}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          e.currentTarget.blur();
        }
      }}
    />
  );
}

function NumberEditor({
  value,
  pending,
  onCommitValue,
  onInvalidJson,
  onClearFeedback,
}: PropertyEditorControlProps): React.JSX.Element {
  const [draft, setDraft] = useState<string>(String(value as number));

  function commitIfChanged(): void {
    if (draft === String(value as number)) return;
    const parsed = Number(draft);
    if (draft.trim() === '' || Number.isNaN(parsed)) {
      // Reuse the invalid-feedback channel for a non-numeric entry.
      onInvalidJson('数値として解釈できません。');
      return;
    }
    onCommitValue(parsed);
  }

  return (
    <input
      className="value value--number prop-editor__input"
      type="number"
      value={draft}
      disabled={pending}
      onChange={(e) => {
        setDraft(e.target.value);
        onClearFeedback();
      }}
      onBlur={commitIfChanged}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          e.currentTarget.blur();
        }
      }}
    />
  );
}

function BooleanEditor({
  value,
  pending,
  onCommitValue,
  onClearFeedback,
}: PropertyEditorControlProps): React.JSX.Element {
  // Boolean commits immediately on toggle (a checkbox has no "blur to commit"
  // affordance); the value is local only between click and the store update.
  return (
    <label className="value value--boolean prop-editor__checkbox">
      <input
        type="checkbox"
        checked={value as boolean}
        disabled={pending}
        onChange={(e) => {
          onClearFeedback();
          onCommitValue(e.target.checked);
        }}
      />
      <span>{(value as boolean) ? 'true' : 'false'}</span>
    </label>
  );
}

// ---- JSON editor (null / array / object) ----------------------------------

function JsonEditor({
  value,
  pending,
  onCommitValue,
  onInvalidJson,
  onClearFeedback,
}: PropertyEditorControlProps): React.JSX.Element {
  // Seed from a pretty-printed serialization of the committed value. The user
  // edits raw JSON text; nothing is dispatched until Apply (JSON.parse here).
  const initial = JSON.stringify(value, null, 2) ?? 'null';
  const [draft, setDraft] = useState<string>(initial);

  function apply(): void {
    let parsed: unknown;
    try {
      parsed = JSON.parse(draft);
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      // Component-local validation error: shown inline, NOT sent to the engine.
      onInvalidJson(`不正な JSON: ${message}`);
      return;
    }
    onCommitValue(parsed as PropertyValue);
  }

  const dirty = draft !== initial;

  return (
    <span className="prop-editor__json">
      <textarea
        className="value__json prop-editor__textarea"
        value={draft}
        disabled={pending}
        spellCheck={false}
        rows={Math.min(8, Math.max(2, draft.split('\n').length))}
        onChange={(e) => {
          setDraft(e.target.value);
          onClearFeedback();
        }}
      />
      <button
        type="button"
        className="prop-editor__apply"
        disabled={pending || !dirty}
        onClick={apply}
      >
        Apply
      </button>
    </span>
  );
}

// ---- inline row feedback ---------------------------------------------------

interface RowFeedbackViewProps {
  feedback: RowFeedback;
  pending: boolean;
}

function RowFeedbackView({ feedback, pending }: RowFeedbackViewProps): React.JSX.Element | null {
  if (pending) {
    return <span className="prop-editor__hint prop-editor__hint--pending">送信中...</span>;
  }
  switch (feedback.kind) {
    case 'none':
      return null;
    case 'invalidJson':
      return (
        <span className="prop-editor__hint prop-editor__hint--error" role="alert">
          {feedback.message}
        </span>
      );
    case 'rejected':
      return (
        <span className="prop-editor__hint prop-editor__hint--error" role="alert">
          エンジンが変更を拒否しました (accepted: false)。
        </span>
      );
    case 'error':
      return (
        <span className="prop-editor__hint prop-editor__hint--error" role="alert">
          送信に失敗しました: {feedback.message}
        </span>
      );
    default: {
      const _exhaustive: never = feedback;
      return <span>{String(_exhaustive)}</span>;
    }
  }
}
