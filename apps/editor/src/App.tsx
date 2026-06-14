import type React from 'react';
import './styles.css';
import { AppLayout }      from './components/AppLayout.js';
import { BridgeProvider } from './state/BridgeContext.js';

function App(): React.JSX.Element {
  return (
    <BridgeProvider>
      <AppLayout />
    </BridgeProvider>
  );
}

export default App;
