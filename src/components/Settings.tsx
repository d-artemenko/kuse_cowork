import { Component, Show, createSignal } from "solid-js";
import { useSettings } from "../stores/settings";
import "./Settings.css";

const Settings: Component = () => {
  const { settings, moltisStatus, updateSetting, toggleSettings, refreshMoltisStatus } = useSettings();
  const [testingMoltis, setTestingMoltis] = createSignal(false);
  const [moltisTestResult, setMoltisTestResult] = createSignal<string | null>(null);

  const handleMoltisTest = async () => {
    setTestingMoltis(true);
    setMoltisTestResult(null);
    try {
      await refreshMoltisStatus();
      const status = moltisStatus();
      if (status.ok) {
        setMoltisTestResult(
          `success (version=${status.version ?? "unknown"}, protocol=${status.protocol ?? 0})`
        );
      } else {
        setMoltisTestResult(`Error: ${status.error || "Moltis is unreachable"}`);
      }
    } catch (e) {
      const errorMsg = e instanceof Error ? e.message : String(e);
      setMoltisTestResult(`Error: ${errorMsg}`);
    } finally {
      setTestingMoltis(false);
    }
  };

  return (
    <div class="settings">
      <div class="settings-header">
        <h2>Settings</h2>
        <button class="close-btn" onClick={toggleSettings}>
          Close
        </button>
      </div>

      <div class="settings-content">
        <div class="settings-section">
          <h3>Moltis Connection</h3>

          <div class="form-group">
            <label for="moltisServerUrl">Moltis Server URL</label>
            <input
              id="moltisServerUrl"
              type="text"
              value={settings().moltisServerUrl}
              onInput={(e) => updateSetting("moltisServerUrl", e.currentTarget.value)}
              placeholder="http://127.0.0.1:13131"
            />
            <span class="hint">Gateway URL used for /health and /ws/chat RPC.</span>
          </div>

          <div class="form-group">
            <label for="moltisApiKey">
              Moltis API Key
              <span class="optional-tag">(Optional)</span>
            </label>
            <input
              id="moltisApiKey"
              type="password"
              value={settings().moltisApiKey}
              onInput={(e) => updateSetting("moltisApiKey", e.currentTarget.value)}
              placeholder="moltis-api-key"
            />
            <span class="hint">Leave empty if Moltis auth is disabled in your environment.</span>
          </div>

          <div class="form-group">
            <button
              class="test-btn"
              onClick={handleMoltisTest}
              disabled={testingMoltis() || !settings().moltisServerUrl.trim()}
            >
              {testingMoltis() ? "Testing Moltis..." : "Test Moltis Connection"}
            </button>
            <Show when={moltisTestResult() && !moltisTestResult()!.startsWith("Error:")}>
              <span class="test-success">✓ {moltisTestResult()}</span>
            </Show>
            <Show when={moltisTestResult() && moltisTestResult()!.startsWith("Error:")}>
              <span class="test-error">{moltisTestResult()}</span>
            </Show>
          </div>

          <div class="form-group">
            <span class={moltisStatus().ok ? "test-success" : "test-error"}>
              {moltisStatus().ok ? "Connected" : "Disconnected"}
              {moltisStatus().error ? `: ${moltisStatus().error}` : ""}
            </span>
            <span class="hint">
              Server: {moltisStatus().serverUrl || settings().moltisServerUrl || "not configured"}
              <br />
              Auth mode: {moltisStatus().authMode}
              <br />
              Protocol: {moltisStatus().protocol ?? "unknown"}
            </span>
          </div>
        </div>

        <div class="settings-section">
          <h3>Data Storage</h3>
          <p class="hint" style={{ margin: 0 }}>
            All data is stored locally in SQLite.
            <br />
            Messages are sent only through your configured Moltis gateway.
          </p>
        </div>
      </div>
    </div>
  );
};

export default Settings;
