// Thin typed wrappers over @tauri-apps/api invoke / listen.
//
// NO command or event name constants are defined here — that is P3's
// responsibility. These wrappers take the name as a parameter so P2 is
// decoupled from the central name module.
//
// Re-exports bridge-types for consumer convenience.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type { UnlistenFn };

/**
 * Invoke a Tauri command by name and return the typed result.
 *
 * @param name - The Tauri command name (provided by the caller; not hardcoded).
 * @param args - Optional named arguments forwarded to the command.
 */
export async function invokeCommand<TResult>(
  name: string,
  args?: Record<string, unknown>,
): Promise<TResult> {
  return invoke<TResult>(name, args);
}

/**
 * Subscribe to a Tauri event by name.
 *
 * Unwraps `event.payload` and forwards it to the handler so callers never
 * need to interact with the raw TauriEvent wrapper.
 *
 * @param name    - The Tauri event name (provided by the caller; not hardcoded).
 * @param handler - Called with the unwrapped payload whenever the event fires.
 * @returns A promise that resolves to an unlisten function.
 */
export async function subscribeEvent<TPayload>(
  name: string,
  handler: (payload: TPayload) => void,
): Promise<UnlistenFn> {
  return listen<TPayload>(name, (event) => {
    handler(event.payload);
  });
}

// Re-export bridge-types for consumer convenience.
export type * from '@norves/bridge-types';

// Re-export IPC name constants and derived types (P3).
export {
  BRIDGE_COMMANDS,
  workspaceOpen,
  workspaceGet,
  workspaceClose,
  sceneCreateObject,
  sceneDeleteObject,
  sceneReparentObject,
  sceneDuplicateObject,
  assetReadManifest,
  assetResolve,
  assetGetManifest,
  type BridgeCommandName,
} from './commands.js';
export {
  BRIDGE_EVENTS,
  type BridgeEventName,
} from './events.js';

// Re-export IPC contract types (P6).
export type {
  AssetEntry,
  AssetManifestResult,
  AssetManifestPayload,
  AssetResolveResult,
  AssetResolveSource,
  AssetResolveStatus,
  ConnectionStatePayload,
  WorkspacePayload,
} from './ipc-types.js';
