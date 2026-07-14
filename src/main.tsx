import React, { useEffect, useMemo, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { open } from "@tauri-apps/plugin-dialog";
import "./styles.css";

type AppConfig = {
  watchFolder: string;
  hasScanKey: boolean;
};

type WatcherLog = {
  level: "info" | "success" | "error";
  message: string;
  path?: string;
};

const defaultConfig: AppConfig = {
  watchFolder: "",
  hasScanKey: false,
};

function App() {
  const [config, setConfig] = useState<AppConfig>(defaultConfig);
  const [activeTab, setActiveTab] = useState<"watcher" | "config">("watcher");
  const [scanKey, setScanKey] = useState("");
  const [password, setPassword] = useState("");
  const [runOnStartup, setRunOnStartup] = useState(false);
  const [running, setRunning] = useState(false);
  const [saving, setSaving] = useState(false);
  const [logs, setLogs] = useState<WatcherLog[]>([]);

  const canStart = useMemo(() => {
    return Boolean(config.watchFolder && config.hasScanKey);
  }, [config]);

  useEffect(() => {
    invoke<AppConfig>("load_config")
      .then((saved) => {
        const nextConfig = { ...defaultConfig, ...saved };
        setConfig(nextConfig);

        if (nextConfig.hasScanKey) {
          invoke("check_api_key_health")
            .then(() => pushLog("success", "Scan key đang hoạt động"))
            .catch((error) => pushLog("error", String(error)));
        }
      })
      .catch((error) => pushLog("error", String(error)));

    invoke<boolean>("watcher_status")
      .then(setRunning)
      .catch(() => setRunning(false));

    isEnabled()
      .then(setRunOnStartup)
      .catch(() => setRunOnStartup(false));

    const unlisten = listen<WatcherLog>("watcher-log", (event) => {
      setLogs((current) => [event.payload, ...current].slice(0, 120));
    });

    return () => {
      unlisten.then((stop) => stop()).catch(() => undefined);
    };
  }, []);

  function updateConfig<K extends keyof AppConfig>(key: K, value: AppConfig[K]) {
    setConfig((current) => ({ ...current, [key]: value }));
  }

  function pushLog(level: WatcherLog["level"], message: string, path?: string) {
    setLogs((current) => [{ level, message, path }, ...current].slice(0, 120));
  }

  async function chooseFolder() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Chọn thư mục output máy scan",
    });

    if (typeof selected === "string") {
      updateConfig("watchFolder", selected);
    }
  }

  async function saveConfig() {
    if (!scanKey.trim()) {
      pushLog("error", "Thiếu scan key");
      return;
    }

    setSaving(true);
    try {
      const saved = await invoke<AppConfig>("save_config", {
        config: {
          ...config,
          scanKey: scanKey.trim(),
          password: password.trim(),
        },
      });
      if (runOnStartup) {
        await enable();
      } else {
        await disable();
      }
      setConfig({ ...defaultConfig, ...saved });
      setScanKey("");
      setPassword("");
      pushLog("success", "Đã lưu cấu hình");
    } catch (error) {
      pushLog("error", String(error));
    } finally {
      setSaving(false);
    }
  }

  async function startWatcher() {
    try {
      await invoke("save_watch_folder", { watchFolder: config.watchFolder });
      await invoke("start_watcher", { watchFolder: config.watchFolder });
      setRunning(true);
      pushLog("success", "Watcher đang chạy");
    } catch (error) {
      pushLog("error", String(error));
    }
  }

  async function stopWatcher() {
    try {
      await invoke("stop_watcher");
      setRunning(false);
      pushLog("info", "Watcher đã dừng");
    } catch (error) {
      pushLog("error", String(error));
    }
  }

  return (
    <main className="app-shell">
      <section className="header">
        <div>
          <p className="eyebrow">LH TSA OCR</p>
          <h1>Scan Watcher</h1>
        </div>
        <span className={running ? "status status-on" : "status"}>{running ? "Running" : "Stopped"}</span>
      </section>

      <section className="panel">
        <div className="tabs" role="tablist" aria-label="App sections">
          <button
            type="button"
            className={activeTab === "watcher" ? "tab tab-active" : "tab"}
            onClick={() => setActiveTab("watcher")}
          >
            Watcher
          </button>
          <button
            type="button"
            className={activeTab === "config" ? "tab tab-active" : "tab"}
            onClick={() => setActiveTab("config")}
          >
            Config
          </button>
        </div>

        {activeTab === "watcher" ? (
          <div className="tab-panel">
            <div className="field">
              <label>Thư mục output máy scan</label>
              <div className="folder-row">
                <input
                  value={config.watchFolder}
                  onChange={(event) => updateConfig("watchFolder", event.target.value)}
                />
                <button type="button" onClick={chooseFolder}>
                  Chọn
                </button>
              </div>
            </div>

            <div className="actions">
              {running ? (
                <button type="button" className="danger" onClick={stopWatcher}>
                  Stop
                </button>
              ) : (
                <button type="button" onClick={startWatcher} disabled={!canStart}>
                  Start watcher
                </button>
              )}
            </div>
          </div>
        ) : (
          <div className="tab-panel">
            <div className="field">
              <label>Scan key</label>
              <input
                type="password"
                value={scanKey}
                onChange={(event) => setScanKey(event.target.value)}
                placeholder={config.hasScanKey ? "Đã lưu trong Stronghold" : "Nhập scan key"}
              />
            </div>

            <div className="field">
              <label>Password lưu cấu hình</label>
              <input
                type="password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                placeholder="Nhập password"
              />
            </div>

            <label className="check-row">
              <input
                type="checkbox"
                checked={runOnStartup}
                onChange={(event) => setRunOnStartup(event.target.checked)}
              />
              <span>Run on startup</span>
            </label>

            <div className="actions">
              <button type="button" className="secondary" onClick={saveConfig} disabled={saving || !password || !scanKey}>
                {saving ? "Đang lưu" : "Lưu config"}
              </button>
            </div>
          </div>
        )}
      </section>

      <section className="panel log-panel">
        <div className="log-header">
          <h2>Log upload</h2>
          <button type="button" className="ghost" onClick={() => setLogs([])}>
            Clear
          </button>
        </div>

        <div className="logs">
          {logs.length === 0 ? (
            <p className="muted">Chưa có log.</p>
          ) : (
            logs.map((log, index) => (
              <div className={`log log-${log.level}`} key={`${log.message}-${index}`}>
                <span>{log.level}</span>
                <p>{log.message}</p>
                {log.path ? <small>{log.path}</small> : null}
              </div>
            ))
          )}
        </div>
      </section>
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
