/**
 * BridgeContext — provides BridgeState and dispatch to the component tree.
 *
 * Wrap the application root with <BridgeProvider>; consume via
 * useBridgeState() / useBridgeDispatch() hooks.
 */

import React, {
  createContext,
  useContext,
  useReducer,
  type Dispatch,
} from 'react';
import { bridgeReducer, INITIAL_STATE, type BridgeState, type BridgeAction } from './store.js';

// -------------------------------------------------------------------------
// Contexts
// -------------------------------------------------------------------------

const BridgeStateContext = createContext<BridgeState | undefined>(undefined);
const BridgeDispatchContext = createContext<Dispatch<BridgeAction> | undefined>(undefined);

// -------------------------------------------------------------------------
// Provider
// -------------------------------------------------------------------------

export interface BridgeProviderProps {
  children: React.ReactNode;
}

export function BridgeProvider({ children }: BridgeProviderProps): React.JSX.Element {
  const [state, dispatch] = useReducer(bridgeReducer, INITIAL_STATE);
  return (
    <BridgeStateContext.Provider value={state}>
      <BridgeDispatchContext.Provider value={dispatch}>
        {children}
      </BridgeDispatchContext.Provider>
    </BridgeStateContext.Provider>
  );
}

// -------------------------------------------------------------------------
// Consumer hooks
// -------------------------------------------------------------------------

export function useBridgeState(): BridgeState {
  const ctx = useContext(BridgeStateContext);
  if (ctx === undefined) {
    throw new Error('useBridgeState must be used inside <BridgeProvider>');
  }
  return ctx;
}

export function useBridgeDispatch(): Dispatch<BridgeAction> {
  const ctx = useContext(BridgeDispatchContext);
  if (ctx === undefined) {
    throw new Error('useBridgeDispatch must be used inside <BridgeProvider>');
  }
  return ctx;
}
