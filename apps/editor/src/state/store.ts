/**
 * UI state store for NorvesEditor — bridge/engine/log state.
 *
 * Architecture: React Context + useReducer (no external state libs).
 * The reducer is pure and fully unit-testable.
 *
 * ConnectionStatus is a UI-level concept derived from:
 *   - ConnectionStatePayload.connected (from Tauri command returns / connection-state event)
 *   - In-flight command state (commandPending / commandSettled actions)
 */

import type {
  EngineState,
  RuntimeState,
  LogLevel,
  ViewportState,
  EngineStatusChangedEvent,
  RuntimeStateChangedEvent,
  LogMessageEvent,
  ErrorReportedEvent,
  EngineProcessExitedEvent,
  ViewportStateChangedEvent,
  SceneTreeChangedEvent,
  ObjectChangedEvent,
  GetStatusResult,
  SceneNode,
  ObjectSnapshot,
  PropertyValue,
  TypeDescriptor,
  ViewportThumbnail,
} from '@norves/bridge-ui';
import type {
  AssetEntry,
  AssetManifestPayload,
  AssetResolveResult,
  ConnectionStatePayload,
  WorkspacePayload,
} from '@norves/bridge-ui';
export type { AssetEntry, AssetManifestPayload } from '@norves/bridge-ui';

// -------------------------------------------------------------------------
// Derived connection status (UI layer concept)
// -------------------------------------------------------------------------

export type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'error';

// -------------------------------------------------------------------------
// Log entry (client-side augmented)
// -------------------------------------------------------------------------

export interface LogEntry {
  /** Monotonic client-side id — used as React key. */
  id: number;
  level: LogLevel;
  message: string;
  category?: string;
  /** ISO-8601 timestamp from the wire, or undefined. */
  timestamp?: string;
}

// -------------------------------------------------------------------------
// Backend error shape (Tauri BackendError serde-tagged)
// -------------------------------------------------------------------------

export interface BackendError {
  kind?: string;
  message: string;
}

// -------------------------------------------------------------------------
// Offline asset manifest helpers
// -------------------------------------------------------------------------

export function assetKeyForEntry(entry: Pick<AssetEntry, 'logicalPath' | 'variant'>): string {
  return JSON.stringify([entry.logicalPath, entry.variant ?? null]);
}

export function findAssetEntryByKey(
  manifest: AssetManifestPayload | undefined,
  key: string | undefined,
): AssetEntry | undefined {
  if (manifest === undefined || key === undefined) {
    return undefined;
  }
  return manifest.assets.find((entry) => assetKeyForEntry(entry) === key);
}

// -------------------------------------------------------------------------
// Scene-edit undo/redo history (Phase U1)
// -------------------------------------------------------------------------

/**
 * A recorded, undoable scene-structure edit. Each variant carries exactly the
 * VALUES (ids/strings/property values) needed to compute an inverse and a redo —
 * never a live engine pointer or a subtree snapshot. Undo is composed from the
 * existing scene/object commands (createObject/duplicateObject/reparentObject/
 * deleteObject/setProperty).
 *
 * Scope (U1): create, duplicate, reparent. A delete is NOT undoable and instead
 * clears both stacks.
 *
 * Added in U2: setProperty (covers rename via the Entity `Name` property and any
 * Property Inspector scalar/array/object edit). It carries only the property's
 * VALUES (oldValue/newValue) as snapshot copies — never a live reference; its
 * inverse re-sets the property to oldValue and its redo re-sets it to newValue
 * (the id is stable across both).
 *
 * Engine-agnostic: all fields are opaque tokens / plain JSON property values.
 */
export type SceneEditCommand =
  | { kind: 'create'; createdId: string; parentId?: string; objectKind?: string }
  | { kind: 'duplicate'; createdId: string; sourceId: string; parentId?: string }
  | { kind: 'reparent'; objectId: string; oldParentId?: string; newParentId?: string }
  | {
      kind: 'setProperty';
      objectId: string;
      property: string;
      oldValue: PropertyValue;
      newValue: PropertyValue;
    };

/**
 * Structural equality for PropertyValue (JSON-ish: scalar/array/object).
 * Mirrors PropertyInspectorPanel's local `stableValueKey` semantics (JSON.stringify
 * comparison) but is a separate implementation — used to skip recording a
 * no-op property edit (old === new). Deliberately not shared with the panel's
 * local helper to avoid a cross-module dependency for a one-line comparison;
 * same JSON.stringify semantics (key-order sensitive, adequate here since both
 * values originate from the same snapshot/engine-echo source).
 */
export function propertyValuesEqual(a: PropertyValue, b: PropertyValue): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/**
 * Finds the id of the parent of `id` within `root`, or undefined when `id` is a
 * direct child of the root (its parent IS the root) / not present at all.
 *
 * The root SceneNode is `state.sceneTree` itself; by convention its direct
 * children are treated as parent = undefined so that an undo of a root-level
 * reparent issues reparentObject(objectId, undefined) — the engine's
 * nullptr=root path — rather than passing the root node's id as a parent (B2).
 *
 * Recursive, same style as mergeChangedNodes. Pure — reads only ids/children.
 */
export function findParentId(root: SceneNode, id: string): string | undefined {
  const children = root.children;
  if (children === undefined) {
    return undefined;
  }
  for (const child of children) {
    if (child.id === id) {
      // Direct child of `root`: only return `root.id` when `root` is a non-root
      // node. At the top level the caller passes the scene root, so a direct
      // child of the scene root yields undefined (B2). We cannot distinguish the
      // scene root from an interior node here, so callers must pass the whole
      // tree and treat the top-level root specially; see normalizeOldParentId.
      return root.id;
    }
    const found = findParentId(child, id);
    if (found !== undefined) {
      return found;
    }
  }
  return undefined;
}

/**
 * Computes the captured oldParentId for an undoable reparent (B1/B2):
 * the current parent's id, or undefined when the object is a direct child of the
 * scene root (undefined == the engine's nullptr=root path) or is not found.
 *
 * `root` is the scene root (state.sceneTree). A direct child of the scene root
 * gets normalized to undefined; any deeper node keeps its interior parent's id.
 */
export function normalizeOldParentId(
  root: SceneNode,
  objectId: string,
): string | undefined {
  const parentId = findParentId(root, objectId);
  // findParentId returns root.id for a direct child of the scene root; normalize
  // that to undefined so undo uses the nullptr=root path rather than the root id.
  if (parentId === root.id) {
    return undefined;
  }
  return parentId;
}

// -------------------------------------------------------------------------
// Store state
// -------------------------------------------------------------------------

export interface BridgeState {
  /** Editor workspace root currently opened by the backend, independent of Bridge connection state. */
  workspace?: WorkspacePayload;
  /**
   * Offline asset manifest loaded from a workspace manifest.json via the
   * backend filesystem command. Independent of Bridge connection state.
   */
  assetManifest?: AssetManifestPayload;
  /** Selected asset key, derived from logicalPath + variant. */
  selectedAssetKey?: string;
  /**
   * Last offline asset-manifest read/parse failure. Kept SEPARATE from
   * `lastError` and runtime reload failures so the Asset Browser never mixes
   * editor-local file errors with Bridge errors. Independent of Bridge
   * connection state.
   */
  assetError?: BackendError;
  /** Runtime asset-manifest reload failure for the current Bridge connection. */
  assetReloadError?: BackendError;
  /** Whether runtime asset-manifest reload is unsupported for the current connection. */
  assetReloadUnsupported: boolean;
  /**
   * Live asset.resolve health results keyed by assetKeyForEntry(logicalPath,
   * variant). This is per Bridge connection and overlays the offline manifest
   * rows; it is never the source of the row set.
   */
  assetResolveByKey?: Record<string, AssetResolveResult>;
  /**
   * Per-connection asset.resolve FAILURES keyed by assetKeyForEntry. A single
   * asset's live health probe failing (e.g. timeout) belongs HERE — surfaced as
   * that row's "未確定" health only — and must NOT touch the shared `assetError`
   * manifest banner, which is reserved for offline manifest read/parse failures.
   */
  assetResolveErrorByKey?: Record<string, BackendError>;
  /**
   * Per-connection asset.resolve capability verdict:
   *   undefined = not probed yet, false = METHOD_NOT_SUPPORTED, true = supported.
   */
  assetCapabilitySupported?: boolean;
  connection: {
    status: ConnectionStatus;
    sessionId?: string;
    serverName?: string;
    endpoint?: string;
    capabilityNames?: ReadonlySet<string>;
    reason?: string;
  };
  engineState?: EngineState;
  runtimeState?: RuntimeState;
  /** Latest viewport state received from the engine. No panel renders this yet (reserved for Workstream K). */
  viewportState?: ViewportState;
  engineName?: string;
  engineVersion?: string;
  title?: string;
  logs: LogEntry[];
  lastError?: BackendError;
  /**
   * ID of the currently selected scene object, or undefined when nothing is selected.
   * Generic string — not tied to any specific engine or mock.
   * Set via objectSelected action; undefined means deselected.
   */
  selectedObjectId?: string;
  /**
   * Root node of the latest scene.getTree snapshot, or undefined before any
   * tree has been fetched (or after disconnect). The wire shape is a single
   * root SceneNode ({ root: SceneNode }); we store the root directly.
   * Generic — carries no engine-specific assumptions.
   */
  sceneTree?: SceneNode;
  /**
   * True when the connected engine answered scene.getTree with
   * METHOD_NOT_SUPPORTED — i.e. it does not implement scene query. This is the
   * engine-agnostic degradation signal the Outliner reads to show an
   * "unsupported" notice instead of an empty scene. Reset on (re)connect.
   */
  sceneUnsupported?: boolean;
  /**
   * True when the connected engine answered scene edit methods with
   * METHOD_NOT_SUPPORTED. Connection-scoped degradation signal for create,
   * delete, and reparent controls.
   */
  sceneEditUnsupported?: boolean;
  /**
   * Editor-side undo history for scene-structure edits (Phase U1; U2 adds
   * setProperty). Each entry is a recorded SceneEditCommand (create/duplicate/
   * reparent/setProperty) whose inverse is composed from the existing scene/
   * object commands. The top of the stack is the most recent edit. Cleared on
   * disconnect / process exit (the ids belong to a dead engine) and by a
   * non-undoable delete. Preserved across workspaceClosed (the Bridge connection
   * persists).
   */
  undoStack: SceneEditCommand[];
  /**
   * Redo history for scene-structure edits (Phase U1). Populated by undo (a
   * popped undoStack entry moves here) and drained by redo. A fresh recorded
   * edit clears it (the classic "new edit invalidates the redo branch" rule).
   * Cleared on the same events as undoStack.
   */
  redoStack: SceneEditCommand[];
  /**
   * Set by a scene.treeChanged live event carrying fullRefreshRequired:true:
   * the incremental changedNodes are insufficient and the Outliner should
   * re-fetch the whole tree via scene.getTree. The Outliner has a consume effect
   * that, while connected, issues getSceneTree() exactly once per set flag; the
   * subsequent sceneTreeLoaded/sceneTreeUnsupported reducer clears it back to
   * false (so the flag is consumed and never re-triggers a loop). Live updates
   * are best-effort; the connect-time + selection-time fetches remain the primary
   * guarantee. Engine-agnostic.
   */
  sceneRefreshRequired?: boolean;
  /**
   * Snapshot of the currently selected object's properties (object.getSnapshot),
   * or undefined when nothing is selected / no snapshot has arrived yet. Cleared
   * on deselect, disconnect, and process exit so a stale object never lingers.
   * Generic — carries no engine-specific assumptions.
   */
  objectSnapshot?: ObjectSnapshot;
  /**
   * Generic type descriptors from schema.getSnapshot, fetched once per
   * connection. Used as an auxiliary hint when rendering property valueTypes.
   * Undefined before any schema has been fetched (or after disconnect).
   */
  schemaTypes?: TypeDescriptor[];
  /**
   * True when the connected engine answered object.getSnapshot (or
   * schema.getSnapshot) with METHOD_NOT_SUPPORTED — i.e. it does not implement
   * object/schema query. Engine-agnostic degradation signal the Inspector reads
   * to show an "unsupported" notice. Reset on (re)connect.
   */
  objectUnsupported?: boolean;
  /**
   * Latest viewport thumbnail (viewport.getThumbnail), or undefined before any
   * thumbnail has been fetched (or after disconnect). The image is an inline
   * base64 snapshot (PNG by default); the GameView renders it as a data: URL.
   * Generic — carries no engine-specific assumptions. Cleared on disconnect /
   * process exit so a stale frame never lingers.
   */
  viewportThumbnail?: ViewportThumbnail;
  /**
   * True when the connected engine answered viewport.getThumbnail with
   * METHOD_NOT_SUPPORTED — i.e. it does not provide thumbnails. Engine-agnostic
   * degradation signal the GameView reads to fall back to the external-window
   * notice instead of showing a thumbnail. Reset on (re)connect.
   */
  viewportThumbnailUnsupported?: boolean;
}

export const INITIAL_STATE: BridgeState = {
  workspace: undefined,
  assetManifest: undefined,
  connection: { status: 'disconnected' },
  logs: [],
  selectedObjectId: undefined,
  selectedAssetKey: undefined,
  assetResolveByKey: undefined,
  assetResolveErrorByKey: undefined,
  assetCapabilitySupported: undefined,
  assetReloadUnsupported: false,
  undoStack: [],
  redoStack: [],
};

// -------------------------------------------------------------------------
// Log cap
// -------------------------------------------------------------------------

const MAX_LOG_ENTRIES = 1000;

// -------------------------------------------------------------------------
// Actions
// -------------------------------------------------------------------------

export type BridgeAction =
  | { type: 'commandPending' }
  | { type: 'commandSettled' }
  | { type: 'workspaceOpened'; payload: WorkspacePayload }
  | { type: 'workspaceClosed' }
  | { type: 'workspaceError'; payload: { error: BackendError } }
  | { type: 'assetManifestLoaded'; payload: AssetManifestPayload }
  | { type: 'assetManifestCleared' }
  | { type: 'assetSelected'; key: string | undefined }
  | { type: 'assetManifestError'; payload: { error: BackendError } }
  | { type: 'assetErrorDismissed' }
  | { type: 'assetReloadSucceeded' }
  | { type: 'assetReloadFailed'; payload: { error: BackendError } }
  | { type: 'assetReloadUnsupported' }
  | { type: 'assetReloadErrorDismissed' }
  | { type: 'assetResolveLoaded'; key: string; result: AssetResolveResult }
  | { type: 'assetResolveError'; key: string; payload: { error: BackendError } }
  | { type: 'assetResolveUnsupported' }
  | { type: 'assetResolveCleared' }
  | { type: 'connectionStateChanged'; payload: ConnectionStatePayload }
  | { type: 'statusUpdated'; payload: GetStatusResult }
  | { type: 'logAppended'; payload: LogMessageEvent; id: number }
  | { type: 'runtimeStateChanged'; payload: RuntimeStateChangedEvent }
  | { type: 'engineStatusChanged'; payload: EngineStatusChangedEvent }
  | { type: 'errorReported'; payload: ErrorReportedEvent }
  | { type: 'engineProcessExited'; payload: EngineProcessExitedEvent }
  | { type: 'viewportStateChanged'; payload: ViewportStateChangedEvent }
  /**
   * A scene.treeChanged live event arrived (protocol 0.2, engine-emitted).
   * Best-effort: when fullRefreshRequired is set the store records a refetch
   * flag for the Outliner; otherwise changedNodes are merged into the existing
   * tree by id. Engine-agnostic — no mock-specific assumptions.
   */
  | { type: 'sceneTreeChangedLive'; payload: SceneTreeChangedEvent }
  /**
   * An object.changed live event arrived (protocol 0.2, engine-emitted). When
   * the changed object is the currently selected one, its in-store snapshot is
   * refreshed with the event's properties/name/kind. Engine-agnostic.
   */
  | { type: 'objectChangedLive'; payload: ObjectChangedEvent }
  | { type: 'dismissError' }
  /**
   * Select a scene object by id. Pass undefined to deselect.
   * Engine-agnostic: id is a plain string token, not mock-specific.
   */
  | { type: 'objectSelected'; id: string | undefined }
  /**
   * Store the root of a freshly fetched scene.getTree snapshot.
   * `root` is the single root SceneNode from the wire result { root }.
   */
  | { type: 'sceneTreeLoaded'; root: SceneNode }
  /**
   * Mark scene query as unsupported by the connected engine (scene.getTree
   * answered METHOD_NOT_SUPPORTED). Engine-agnostic degradation signal.
   */
  | { type: 'sceneTreeUnsupported' }
  /** Mark scene edit methods as unsupported by the connected engine. */
  | { type: 'sceneEditUnsupported' }
  /** Clear selected object state after an accepted scene.deleteObject result. */
  | { type: 'sceneObjectDeleted'; accepted: boolean }
  /**
   * Store a freshly fetched object.getSnapshot for the selected object.
   * Engine-agnostic: a generic property bag, not mock-specific.
   */
  | { type: 'objectSnapshotLoaded'; snapshot: ObjectSnapshot }
  /**
   * Store the type descriptors from a schema.getSnapshot fetch.
   */
  | { type: 'schemaSnapshotLoaded'; types: TypeDescriptor[] }
  /**
   * Mark object/schema query as unsupported by the connected engine
   * (object.getSnapshot or schema.getSnapshot answered METHOD_NOT_SUPPORTED).
   * Engine-agnostic degradation signal.
   */
  | { type: 'objectSnapshotUnsupported' }
  /**
   * Apply an accepted object.setProperty result to the in-store snapshot: the
   * named property's value is replaced with the engine's appliedValue so the
   * Inspector reflects what the engine actually stored (which may be normalized).
   * Scoped to objectId so a late ack for a no-longer-selected object cannot
   * clobber the current snapshot. Engine-agnostic: property/value are generic.
   */
  | {
      type: 'objectPropertyApplied';
      objectId: string;
      property: string;
      appliedValue: PropertyValue;
    }
  /**
   * Store a freshly fetched viewport.getThumbnail result (pull-style). Replaces
   * any previous thumbnail. Engine-agnostic: a generic base64 image + mimeType.
   */
  | { type: 'viewportThumbnailLoaded'; thumbnail: ViewportThumbnail }
  /**
   * Mark viewport thumbnail as unsupported by the connected engine
   * (viewport.getThumbnail answered METHOD_NOT_SUPPORTED). Engine-agnostic
   * degradation signal: the GameView falls back to the external-window notice.
   */
  | { type: 'viewportThumbnailUnsupported' }
  // --- Scene-edit undo/redo history (Phase U1) ---
  /**
   * Record a freshly accepted scene edit onto the undo stack and CLEAR the redo
   * stack (a new edit invalidates the redo branch). Dispatched by the public
   * scene-edit wrappers on accepted:true only.
   */
  | { type: 'recordSceneEdit'; command: SceneEditCommand }
  /**
   * Commit a successful undo: pop the undoStack top and push it onto redoStack.
   * The entry is moved unchanged (reparent ids are stable across undo).
   */
  | { type: 'undoCommitted' }
  /**
   * Commit a successful redo: pop the redoStack top and push it onto undoStack.
   * For a create/duplicate whose re-created object got a NEW id, `newId` replaces
   * the entry's createdId so a subsequent undo deletes the new id (id-instability
   * fix). For a reparent or setProperty (both id-stable) `newId` is omitted and
   * the entry is pushed back unchanged.
   */
  | { type: 'redoCommitted'; newId?: string }
  /**
   * A failed undo (engine returned accepted:false or an error): drop the
   * undoStack top and surface `message` via lastError. The entry is dropped
   * because its inverse could not be applied (e.g. a stale id, out of U1 scope).
   */
  | { type: 'undoFailed'; message: string }
  /**
   * A failed redo: drop the redoStack top and surface `message` via lastError.
   */
  | { type: 'redoFailed'; message: string }
  /**
   * Clear BOTH undo and redo stacks. Dispatched by a non-undoable delete (delete
   * is not undoable in U1, so any recorded history becomes unreconstructable).
   */
  | { type: 'sceneEditHistoryCleared' };

// -------------------------------------------------------------------------
// Scene-tree merge helper (for scene.treeChanged live events)
// -------------------------------------------------------------------------

/**
 * Returns a new tree where any node whose id appears in `changes` is replaced by
 * the changed node (a full snapshot DTO). Recurses into children. Pure: returns
 * the same reference when nothing matched, so an unrelated live event cannot
 * force a re-render. Engine-agnostic — operates purely on generic id/children.
 */
function mergeChangedNodes(
  node: SceneNode,
  changes: Map<string, SceneNode>,
): SceneNode {
  const replacement = changes.get(node.id);
  if (replacement !== undefined) {
    // A changed node is a full subtree snapshot; replace wholesale.
    return replacement;
  }
  if (node.children === undefined || node.children.length === 0) {
    return node;
  }
  let childChanged = false;
  const children = node.children.map((child) => {
    const next = mergeChangedNodes(child, changes);
    if (next !== child) {
      childChanged = true;
    }
    return next;
  });
  if (!childChanged) {
    return node;
  }
  return { ...node, children };
}

// -------------------------------------------------------------------------
// Pure reducer
// -------------------------------------------------------------------------

export function bridgeReducer(state: BridgeState, action: BridgeAction): BridgeState {
  switch (action.type) {
    case 'commandPending': {
      // Only move to 'connecting' if currently disconnected;
      // avoid overwriting 'error' or 'connected' for non-connect commands.
      if (state.connection.status === 'disconnected') {
        return {
          ...state,
          connection: { ...state.connection, status: 'connecting' },
        };
      }
      return state;
    }

    case 'commandSettled': {
      // commandSettled is used to roll back optimistic 'connecting' status
      // when a command returns synchronously (connection-state event takes over).
      return state;
    }

    case 'workspaceOpened': {
      const workspaceChanged = state.workspace?.rootPath !== action.payload.rootPath;
      return {
        ...state,
        workspace: action.payload,
        assetManifest: workspaceChanged ? undefined : state.assetManifest,
        selectedAssetKey: workspaceChanged ? undefined : state.selectedAssetKey,
        assetResolveByKey: workspaceChanged ? undefined : state.assetResolveByKey,
        assetResolveErrorByKey: workspaceChanged ? undefined : state.assetResolveErrorByKey,
        assetCapabilitySupported: workspaceChanged ? undefined : state.assetCapabilitySupported,
      };
    }

    case 'workspaceClosed': {
      return {
        ...state,
        workspace: undefined,
        assetManifest: undefined,
        selectedAssetKey: undefined,
        assetResolveByKey: undefined,
        assetResolveErrorByKey: undefined,
        assetCapabilitySupported: undefined,
      };
    }

    case 'workspaceError': {
      // Workspace failures are editor-local (filesystem) state, independent of
      // the Bridge connection. Surface them via lastError ONLY — never flip
      // connection.status, or an invalid workspace path would wrongly show the
      // engine connection as errored and enable Reconnect.
      return { ...state, lastError: action.payload.error };
    }

    case 'assetManifestLoaded': {
      const selectedStillExists =
        findAssetEntryByKey(action.payload, state.selectedAssetKey) !== undefined;
      return {
        ...state,
        assetManifest: action.payload,
        selectedAssetKey: selectedStillExists ? state.selectedAssetKey : undefined,
        assetResolveByKey: undefined,
        assetResolveErrorByKey: undefined,
        // A successful load clears any prior asset error.
        assetError: undefined,
      };
    }

    case 'assetManifestCleared': {
      return {
        ...state,
        assetManifest: undefined,
        selectedAssetKey: undefined,
        assetError: undefined,
        assetResolveByKey: undefined,
        assetResolveErrorByKey: undefined,
        assetCapabilitySupported: undefined,
      };
    }

    case 'assetSelected': {
      return { ...state, selectedAssetKey: action.key };
    }

    case 'assetManifestError': {
      // Asset failures are editor-local (filesystem) and surface ONLY via the
      // dedicated assetError field — never lastError or connection.status. This
      // keeps a failed reload visible in the Asset Browser even while a prior
      // manifest is still shown, without polluting engine/workspace error UI.
      return { ...state, assetError: action.payload.error };
    }

    case 'assetErrorDismissed': {
      return { ...state, assetError: undefined };
    }

    case 'assetReloadSucceeded': {
      return { ...state, assetReloadError: undefined, assetReloadUnsupported: false };
    }

    case 'assetReloadFailed': {
      return { ...state, assetReloadError: action.payload.error };
    }

    case 'assetReloadUnsupported': {
      return { ...state, assetReloadUnsupported: true };
    }

    case 'assetReloadErrorDismissed': {
      return { ...state, assetReloadError: undefined };
    }

    case 'assetResolveLoaded': {
      const nextErrors = { ...(state.assetResolveErrorByKey ?? {}) };
      delete nextErrors[action.key];
      return {
        ...state,
        assetResolveByKey: {
          ...(state.assetResolveByKey ?? {}),
          [action.key]: action.result,
        },
        assetResolveErrorByKey:
          Object.keys(nextErrors).length > 0 ? nextErrors : undefined,
        assetCapabilitySupported: true,
      };
    }

    case 'assetResolveError': {
      // A single asset's live health probe failed. Record it per-key so the row
      // shows "未確定"; do NOT touch assetError (the manifest banner) or the
      // connection. Drop any stale success for this key.
      const nextResolved = { ...(state.assetResolveByKey ?? {}) };
      delete nextResolved[action.key];
      return {
        ...state,
        assetResolveByKey:
          Object.keys(nextResolved).length > 0 ? nextResolved : undefined,
        assetResolveErrorByKey: {
          ...(state.assetResolveErrorByKey ?? {}),
          [action.key]: action.payload.error,
        },
      };
    }

    case 'assetResolveUnsupported': {
      return {
        ...state,
        assetResolveByKey: undefined,
        assetResolveErrorByKey: undefined,
        assetCapabilitySupported: false,
      };
    }

    case 'assetResolveCleared': {
      return {
        ...state,
        assetResolveByKey: undefined,
        assetResolveErrorByKey: undefined,
        assetCapabilitySupported: undefined,
      };
    }

    case 'connectionStateChanged': {
      const p = action.payload;
      const status: ConnectionStatus = p.connected ? 'connected' : 'disconnected';
      const connectionChanged =
        state.connection.status !== status || state.connection.sessionId !== p.sessionId;
      const capabilityNames = p.capabilities !== undefined
        ? new Set(p.capabilities.map((descriptor) => descriptor.name))
        : p.connected && !connectionChanged
          ? state.connection.capabilityNames
          : undefined;
      return {
        ...state,
        connection: {
          status,
          sessionId: p.sessionId,
          serverName: p.serverName,
          endpoint: p.endpoint,
          capabilityNames,
          reason: p.reason,
        },
        // Clear lastError on successful connection
        lastError: p.connected ? undefined : state.lastError,
        // Live asset.resolve health is scoped to a Bridge connection. A fresh
        // connection re-probes support; disconnect drops the overlay while the
        // offline manifest and selection remain in place.
        assetResolveByKey: p.connected && !connectionChanged ? state.assetResolveByKey : undefined,
        assetResolveErrorByKey:
          p.connected && !connectionChanged ? state.assetResolveErrorByKey : undefined,
        assetCapabilitySupported:
          p.connected && !connectionChanged ? state.assetCapabilitySupported : undefined,
        assetReloadError:
          p.connected && !connectionChanged ? state.assetReloadError : undefined,
        assetReloadUnsupported:
          p.connected && !connectionChanged ? state.assetReloadUnsupported : false,
        // Drop the scene snapshot + selection on disconnect so the Outliner
        // degrades to its disconnected empty state and stale ids cannot linger.
        // A fresh connection also clears any prior "unsupported" marker so the
        // next engine is re-probed.
        sceneTree: p.connected ? state.sceneTree : undefined,
        selectedObjectId: p.connected ? state.selectedObjectId : undefined,
        sceneUnsupported: p.connected ? false : state.sceneUnsupported,
        sceneEditUnsupported: p.connected ? false : state.sceneEditUnsupported,
        // The undo/redo history references object ids owned by the connected
        // engine. A disconnect makes those ids meaningless, so drop both stacks;
        // a fresh connection also starts with an empty history. (A reconnect to
        // the same session keeps the tree above, but scene ids are not guaranteed
        // stable across the transport drop, so we clear conservatively.)
        undoStack: p.connected ? state.undoStack : [],
        redoStack: p.connected ? state.redoStack : [],
        // A pending live-refresh request is meaningless across a (dis)connect.
        sceneRefreshRequired: p.connected ? state.sceneRefreshRequired : undefined,
        // Inspector data is per-object / per-engine: a fresh connection re-probes
        // both, and a disconnect drops the stale snapshot + schema + verdict.
        objectSnapshot: p.connected ? state.objectSnapshot : undefined,
        schemaTypes: p.connected ? state.schemaTypes : undefined,
        objectUnsupported: p.connected ? false : state.objectUnsupported,
        // The viewport thumbnail is per-connection: a fresh connection re-probes
        // it, and a disconnect drops the stale frame + verdict.
        viewportThumbnail: p.connected ? state.viewportThumbnail : undefined,
        viewportThumbnailUnsupported: p.connected ? false : state.viewportThumbnailUnsupported,
      };
    }

    case 'statusUpdated': {
      const p = action.payload;
      return {
        ...state,
        engineState: p.engineState,
        runtimeState: p.runtimeState,
        engineName: p.engineName,
        engineVersion: p.engineVersion,
        title: p.title,
      };
    }

    case 'logAppended': {
      const p = action.payload;
      const entry: LogEntry = {
        id: action.id,
        level: p.level,
        message: p.message,
        category: p.category,
        timestamp: p.timestamp,
      };
      const logs = state.logs.length >= MAX_LOG_ENTRIES
        ? [...state.logs.slice(state.logs.length - MAX_LOG_ENTRIES + 1), entry]
        : [...state.logs, entry];
      return { ...state, logs };
    }

    case 'runtimeStateChanged': {
      return { ...state, runtimeState: action.payload.state };
    }

    case 'engineStatusChanged': {
      const p = action.payload;
      return {
        ...state,
        engineState: p.engineState,
        runtimeState: p.runtimeState ?? state.runtimeState,
        title: p.title ?? state.title,
      };
    }

    case 'errorReported': {
      const err = action.payload.error;
      return {
        ...state,
        lastError: { kind: err.code, message: err.message },
        connection: { ...state.connection, status: 'error' },
      };
    }

    case 'engineProcessExited': {
      return {
        ...state,
        engineState: undefined,
        runtimeState: undefined,
        connection: { ...state.connection, status: 'disconnected', capabilityNames: undefined },
        // The engine is gone: its scene snapshot + selection are no longer valid.
        sceneTree: undefined,
        selectedObjectId: undefined,
        sceneUnsupported: undefined,
        sceneEditUnsupported: undefined,
        sceneRefreshRequired: undefined,
        // The engine is gone: its scene ids (and thus the undo/redo history) are
        // no longer valid, so drop both stacks.
        undoStack: [],
        redoStack: [],
        // The Inspector data is likewise invalid once the engine dies.
        objectSnapshot: undefined,
        schemaTypes: undefined,
        objectUnsupported: undefined,
        // The viewport thumbnail (and its verdict) is invalid once the engine dies.
        viewportThumbnail: undefined,
        viewportThumbnailUnsupported: undefined,
        // Live asset.resolve health is invalid once the engine dies; offline
        // manifest and selectedAssetKey stay editor-local and are preserved.
        assetResolveByKey: undefined,
        assetResolveErrorByKey: undefined,
        assetCapabilitySupported: undefined,
        assetReloadError: undefined,
        assetReloadUnsupported: false,
      };
    }

    case 'viewportStateChanged': {
      return { ...state, viewportState: action.payload.state };
    }

    case 'sceneTreeChangedLive': {
      // Best-effort live tree update (protocol 0.2). fullRefreshRequired asks the
      // Outliner to re-fetch the whole tree; record the flag and leave the stale
      // tree in place until the refetch lands.
      const p = action.payload;
      if (p.fullRefreshRequired === true) {
        return { ...state, sceneRefreshRequired: true };
      }
      // Without a tree in store yet (e.g. event before the first fetch), there is
      // nothing to merge into; the connect-time fetch is the primary guarantee.
      if (state.sceneTree === undefined) {
        return state;
      }
      const changedNodes = p.changedNodes;
      if (changedNodes === undefined || changedNodes.length === 0) {
        return state;
      }
      const changes = new Map<string, SceneNode>();
      for (const n of changedNodes) {
        changes.set(n.id, n);
      }
      const nextTree = mergeChangedNodes(state.sceneTree, changes);
      if (nextTree === state.sceneTree) {
        return state;
      }
      return { ...state, sceneTree: nextTree };
    }

    case 'objectChangedLive': {
      // Best-effort live object update (protocol 0.2). Only refresh the snapshot
      // when the changed object is the one currently shown in the Inspector; a
      // change to any other object is ignored (the connect/selection fetch is the
      // primary guarantee).
      const p = action.payload;
      const snapshot = state.objectSnapshot;
      if (snapshot === undefined || snapshot.objectId !== p.objectId) {
        return state;
      }
      return {
        ...state,
        objectSnapshot: {
          objectId: p.objectId,
          name: p.name ?? snapshot.name,
          kind: p.kind ?? snapshot.kind,
          properties: p.properties,
        },
      };
    }

    case 'dismissError': {
      return { ...state, lastError: undefined };
    }

    case 'objectSelected': {
      // Changing (or clearing) the selection invalidates the previously fetched
      // object snapshot. The schema (engine-wide) is left intact. The fetch
      // effect in the Inspector loads the new selection's snapshot; until it
      // arrives the panel shows a loading state rather than the old object.
      if (action.id === state.selectedObjectId) {
        // Re-selecting the same id is a no-op for the snapshot.
        return { ...state, selectedObjectId: action.id };
      }
      return { ...state, selectedObjectId: action.id, objectSnapshot: undefined };
    }

    case 'sceneTreeLoaded': {
      // A successful tree clears any prior "unsupported" marker and consumes any
      // pending live-refresh request (sceneRefreshRequired -> false), which is
      // what stops the Outliner's consume effect from re-firing.
      return {
        ...state,
        sceneTree: action.root,
        sceneUnsupported: false,
        sceneRefreshRequired: false,
      };
    }

    case 'sceneTreeUnsupported': {
      // No tree to show; record the engine's degradation for the Outliner. Also
      // consumes any pending live-refresh request so the consume effect settles.
      return {
        ...state,
        sceneTree: undefined,
        sceneUnsupported: true,
        sceneRefreshRequired: false,
      };
    }

    case 'sceneEditUnsupported': {
      return { ...state, sceneEditUnsupported: true };
    }

    case 'sceneObjectDeleted': {
      if (!action.accepted) {
        return state;
      }
      return { ...state, selectedObjectId: undefined, objectSnapshot: undefined };
    }

    case 'objectSnapshotLoaded': {
      // A successful snapshot clears any prior "unsupported" marker.
      return { ...state, objectSnapshot: action.snapshot, objectUnsupported: false };
    }

    case 'schemaSnapshotLoaded': {
      return { ...state, schemaTypes: action.types };
    }

    case 'objectSnapshotUnsupported': {
      // No snapshot to show; record the engine's degradation for the Inspector.
      return { ...state, objectSnapshot: undefined, objectUnsupported: true };
    }

    case 'objectPropertyApplied': {
      // Apply an accepted write to the stored snapshot so the Inspector shows the
      // engine's appliedValue without a full re-fetch. Guard on objectId so a
      // late ack for a different object cannot clobber the current snapshot; if
      // the snapshot is gone (deselect/disconnect) or the property is absent,
      // leave state unchanged.
      const snapshot = state.objectSnapshot;
      if (snapshot === undefined || snapshot.objectId !== action.objectId) {
        return state;
      }
      let changed = false;
      const properties = snapshot.properties.map((entry) => {
        if (entry.name === action.property) {
          changed = true;
          return { ...entry, value: action.appliedValue };
        }
        return entry;
      });
      if (!changed) {
        return state;
      }
      return { ...state, objectSnapshot: { ...snapshot, properties } };
    }

    case 'viewportThumbnailLoaded': {
      // A successful thumbnail clears any prior "unsupported" marker.
      return {
        ...state,
        viewportThumbnail: action.thumbnail,
        viewportThumbnailUnsupported: false,
      };
    }

    case 'viewportThumbnailUnsupported': {
      // No thumbnail to show; record the engine's degradation for the GameView.
      return {
        ...state,
        viewportThumbnail: undefined,
        viewportThumbnailUnsupported: true,
      };
    }

    case 'recordSceneEdit': {
      // A fresh accepted edit goes on top of the undo stack and clears the redo
      // branch (the classic "new edit invalidates redo" rule).
      return {
        ...state,
        undoStack: [...state.undoStack, action.command],
        redoStack: [],
      };
    }

    case 'undoCommitted': {
      // Move the most recent edit from the undo stack to the redo stack. Guard an
      // empty stack (a no-op undo should never dispatch this, but stay pure).
      if (state.undoStack.length === 0) {
        return state;
      }
      const top = state.undoStack[state.undoStack.length - 1]!;
      return {
        ...state,
        undoStack: state.undoStack.slice(0, -1),
        redoStack: [...state.redoStack, top],
      };
    }

    case 'redoCommitted': {
      // Move the most recent undone edit back onto the undo stack. For a
      // create/duplicate the re-created object has a NEW id, so replace createdId
      // with action.newId before pushing — a subsequent undo then deletes the new
      // id, not the stale one (id-instability fix). A reparent or setProperty is
      // id-stable, so it falls through the guard below and is pushed unchanged.
      if (state.redoStack.length === 0) {
        return state;
      }
      const top = state.redoStack[state.redoStack.length - 1]!;
      let restored: SceneEditCommand = top;
      if (
        action.newId !== undefined &&
        (top.kind === 'create' || top.kind === 'duplicate')
      ) {
        restored = { ...top, createdId: action.newId };
      }
      return {
        ...state,
        redoStack: state.redoStack.slice(0, -1),
        undoStack: [...state.undoStack, restored],
      };
    }

    case 'undoFailed': {
      // The inverse could not be applied (stale id / engine rejection). Drop the
      // offending entry and surface the reason via the shared lastError.
      return {
        ...state,
        undoStack: state.undoStack.slice(0, -1),
        lastError: { message: action.message },
      };
    }

    case 'redoFailed': {
      return {
        ...state,
        redoStack: state.redoStack.slice(0, -1),
        lastError: { message: action.message },
      };
    }

    case 'sceneEditHistoryCleared': {
      return { ...state, undoStack: [], redoStack: [] };
    }

    default: {
      // Exhaustiveness guard — TypeScript will catch missing cases at compile time.
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}
