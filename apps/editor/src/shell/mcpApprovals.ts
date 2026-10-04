/** 承認用IPCの既存ラッパーとDTOをmain画面へ接続する。 */
export {
  getMcpConfirmations,
  approveMcpConfirmation,
  rejectMcpConfirmation,
} from '../../../../bridge/ts/packages/bridge-ui/src/commands.js';
export type { McpConfirmationRequest } from '../../../../bridge/ts/packages/bridge-ui/src/ipc-types.js';
