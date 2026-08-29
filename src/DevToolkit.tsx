import { useState } from 'react';
import { emit } from '@tauri-apps/api/event';
import './DevToolkit.css';

export default function DevToolkit() {
  if (!import.meta.env.DEV) return null;

  const [isOpen, setIsOpen] = useState(false);

  const mockRecordingStart = () => {
    emit('cue://recording-started', 'Voice');
  };

  const mockRecordingStop = () => {
    emit('cue://stopped', { filename: 'mock.wav', path: 'mock/path.wav' });
  };

  const resetSettings = () => {
    console.log("Settings reset (mock)");
  };

  const testOnboarding = () => {
    console.log("Testing onboarding... (todo)");
  };

  return (
    <div className={`dev-toolkit ${isOpen ? 'open' : ''}`}>
      {!isOpen ? (
        <button className="dev-toolkit-toggle" onClick={() => setIsOpen(true)}>
          🛠️
        </button>
      ) : (
        <div className="dev-toolkit-panel">
          <div className="dev-toolkit-header">
            <h3>Dev Tools</h3>
            <button className="dev-toolkit-close" onClick={() => setIsOpen(false)}>×</button>
          </div>
          <div className="dev-toolkit-actions">
            <button onClick={mockRecordingStart}>Mock Start Rec</button>
            <button onClick={mockRecordingStop}>Mock Stop Rec</button>
            <button onClick={resetSettings}>Reset Settings</button>
            <button onClick={testOnboarding}>Test Onboarding</button>
          </div>
        </div>
      )}
    </div>
  );
}
